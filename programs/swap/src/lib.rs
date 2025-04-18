use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};
use anchor_spl::associated_token::AssociatedToken;

// Program ID and constants
declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLaS");
const JUPITER_PROGRAM_ID: Pubkey = pubkey!("JUP6i4ozu5ydDCnLiMogSckDPpbtr7BJ4FtzYWkb5Rk");
const USDC_MINT: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

// Commission shares
const REFERRAL_SHARE_NUM: u64 = 4;
const ADMIN_SHARE_NUM:    u64 = 6;
const SHARE_DENOM:        u64 = 10;

#[program]
pub mod swap {
    use super::*;

    /// Initialize the swap account, storing admin, referral, and bump
    pub fn initialize(
        ctx: Context<Initialize>,
        admin:   Pubkey,
        referral: Pubkey,
    ) -> Result<()> {
        require_keys_neq!(admin, referral, SwapError::InvalidInput);
        let swap_acc = &mut ctx.accounts.swap_account;
        swap_acc.admin    = admin;
        swap_acc.referral = referral;
        swap_acc.bump     = *ctx.bumps.get("swap_account").unwrap();
        Ok(())
    }

    /// Execute token swap with 1% commission, split between referral and admin
    pub fn swap_tokens(
        ctx: Context<SwapTokens>,
        input_amount:     u64,
        min_output_amount: u64,
    ) -> Result<()> {
        // Basic input validation
        require!(input_amount > 0 && min_output_amount > 0, SwapError::InvalidInput);
        let user_in = &ctx.accounts.user_token_in_account;
        require!(user_in.amount >= input_amount, SwapError::InsufficientBalance);

        // Calculate 1% commission
        let commission = input_amount
            .checked_div(100)
            .ok_or(SwapError::Overflow)?;
        let amount_after = input_amount
            .checked_sub(commission)
            .ok_or(SwapError::Overflow)?;

        // PDA seeds for signing
        let signer_seeds = &[&[b"swap", &[ctx.accounts.swap_account.bump]]];

        // 1) Main swap for user via Jupiter CPI
        let cpi_swap_ctx = CpiContext::new_with_signer(
            ctx.accounts.jupiter_program.to_account_info(),
            jupiter_cpi::accounts::JupiterSwap {
                swap_in:   ctx.accounts.swap_token_in_account.to_account_info(),
                swap_out:  ctx.accounts.user_token_out_account.to_account_info(),
                authority: ctx.accounts.swap_account.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            },
            signer_seeds,
        );
        jupiter_cpi::cpi::swap(cpi_swap_ctx, amount_after, min_output_amount)?;

        // 2) Swap commission portion into USDC via Jupiter CPI
        let cpi_comm_ctx = CpiContext::new_with_signer(
            ctx.accounts.jupiter_program.to_account_info(),
            jupiter_cpi::accounts::JupiterSwap {
                swap_in:   ctx.accounts.swap_token_in_account.to_account_info(),
                swap_out:  ctx.accounts.commission_usdc_account.to_account_info(),
                authority: ctx.accounts.swap_account.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            },
            signer_seeds,
        );
        jupiter_cpi::cpi::swap(cpi_comm_ctx, commission, 0)?;

        // 3) Distribute USDC to referral (40%) and admin (60%)
        let total_usdc       = ctx.accounts.commission_usdc_account.amount;
        let referral_amount  = share(total_usdc, REFERRAL_SHARE_NUM, SHARE_DENOM)?;
        let admin_amount     = total_usdc.checked_sub(referral_amount).ok_or(SwapError::Overflow)?;

        // Transfer to referral
        let cpi_ref_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from:      ctx.accounts.commission_usdc_account.to_account_info(),
                to:        ctx.accounts.referral_usdc_account.to_account_info(),
                authority: ctx.accounts.swap_account.to_account_info(),
            },
            signer_seeds,
        );
        token::transfer(cpi_ref_ctx, referral_amount)?;

        // Transfer to admin
        let cpi_admin_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from:      ctx.accounts.commission_usdc_account.to_account_info(),
                to:        ctx.accounts.admin_usdc_account.to_account_info(),
                authority: ctx.accounts.swap_account.to_account_info(),
            },
            signer_seeds,
        );
        token::transfer(cpi_admin_ctx, admin_amount)?;

        Ok(())
    }
}

// Accounts for initialization
#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = admin,
        space = 8 + 32 + 32 + 1, // discriminator + admin + referral + bump
        seeds = [b"swap"],
        bump
    )]
    pub swap_account: Account<'info, SwapAccount>,

    #[account(mut)]
    pub admin: Signer<'info>,

    /// CHECK: only stored in state
    pub referral: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

// Accounts for swap execution
#[derive(Accounts)]
pub struct SwapTokens<'info> {
    #[account(
        mut,
        seeds = [b"swap"],
        bump = swap_account.bump
    )]
    pub swap_account: Account<'info, SwapAccount>,

    #[account(mut)]
    pub user: Signer<'info>,

    pub token_in_mint: Account<'info, Mint>,
    pub token_out_mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = user_token_in_account.mint == token_in_mint.key() &&
                     user_token_in_account.owner == user.key()
    )]
    pub user_token_in_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = user_token_out_account.mint == token_out_mint.key() &&
                     user_token_out_account.owner == user.key()
    )]
    pub user_token_out_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        associated_token::mint = token_in_mint,
        associated_token::authority = swap_account
    )]
    pub swap_token_in_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        associated_token::mint = USDC_MINT,
        associated_token::authority = swap_account
    )]
    pub commission_usdc_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        associated_token::mint = USDC_MINT,
        associated_token::authority = swap_account.referral
    )]
    pub referral_usdc_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        associated_token::mint = USDC_MINT,
        associated_token::authority = swap_account.admin
    )]
    pub admin_usdc_account: Account<'info, TokenAccount>,

    #[account(constraint = usdc_mint.key() == USDC_MINT)]
    pub usdc_mint: Account<'info, Mint>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,

    pub jupiter_program: Program<'info, Any>,

    /// CHECK: Jupiter route not validated by Anchor
    pub jupiter_route: UncheckedAccount<'info>,
}

// State for swap account
#[account]
pub struct SwapAccount {
    pub admin:    Pubkey,
    pub referral: Pubkey,
    pub bump:     u8,
}

// Helper to calculate shares safely
fn share(amount: u64, num: u64, den: u64) -> Result<u64> {
    amount.checked_mul(num)
        .and_then(|v| v.checked_div(den))
        .ok_or(SwapError::Overflow)
}

// Custom error codes
#[error_code]
pub enum SwapError {
    #[msg("Invalid input")]
    InvalidInput,
    #[msg("Overflow")]
    Overflow,
    #[msg("Insufficient balance")]
    InsufficientBalance,
}

// CPI wrapper for Jupiter Program
pub mod jupiter_cpi {
    use super::*;
    use anchor_lang::solana_program::{
        instruction::{AccountMeta, Instruction},
        program::invoke_signed,
    };

    pub mod cpi {
        use super::*;

        /// Perform a token swap via Jupiter
        pub fn swap<'info>(
            ctx: CpiContext<'_, '_, '_, 'info, accounts::JupiterSwap<'info>>,
            amount_in:  u64,
            minimum_out: u64,
        ) -> Result<()> {
            let ix = Instruction {
                program_id: ctx.program.key(),
                accounts: vec![
                    AccountMeta::new(ctx.accounts.swap_in.key(), false),
                    AccountMeta::new(ctx.accounts.swap_out.key(), false),
                    AccountMeta::new_readonly(ctx.accounts.authority.key(), true),
                    AccountMeta::new_readonly(ctx.accounts.token_program.key(), false),
                ],
                data: {
                    let mut buf = Vec::with_capacity(1 + 8 + 8);
                    buf.push(0u8);
                    buf.extend_from_slice(&amount_in.to_le_bytes());
                    buf.extend_from_slice(&minimum_out.to_le_bytes());
                    buf
                },
            };
            invoke_signed(
                &ix,
                &[
                    ctx.accounts.swap_in.to_account_info(),
                    ctx.accounts.swap_out.to_account_info(),
                    ctx.accounts.authority.to_account_info(),
                    ctx.accounts.token_program.to_account_info(),
                ],
                ctx.signer_seeds,
            )?;
            Ok(())
        }
    }

    pub mod accounts {
        use super::*;

        #[derive(Accounts)]
        pub struct JupiterSwap<'info> {
            #[account(mut)]
            pub swap_in:   Account<'info, TokenAccount>,
            #[account(mut)]
            pub swap_out:  Account<'info, TokenAccount>,
            /// CHECK: PDA authority signing
            pub authority: AccountInfo<'info>,
            pub token_program: Program<'info, Token>,
        }
    }
}
