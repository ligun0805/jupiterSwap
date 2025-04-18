use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount};
use anchor_spl::associated_token::AssociatedToken;

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

/// Jupiter program ID on mainnet
pub const JUPITER_PROGRAM_ID: Pubkey = pubkey!("JUP6i4ozu5ydDCnLiMogSckDPpbtr7BJ4FtzYWkb5Rk");

/// USDC mint address on mainnet
pub const USDC_MINT: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

/// Custom errors for the swap program
#[error_code]
pub enum SwapError {
    #[msg("Invalid admin address")]
    InvalidAdmin,
    #[msg("Invalid referral address")]
    InvalidReferral,
    #[msg("Invalid amount")]
    InvalidAmount,
    #[msg("Invalid minimum output amount")]
    InvalidMinOutAmount,
    #[msg("Commission calculation overflow")]
    CommissionOverflow,
    #[msg("Slippage tolerance exceeded")]
    SlippageExceeded,
    #[msg("Jupiter swap failed")]
    JupiterSwapFailed,
    #[msg("Invalid Jupiter route")]
    InvalidJupiterRoute,
    #[msg("Insufficient balance")]
    InsufficientBalance,
    #[msg("Invalid token account")]
    InvalidTokenAccount,
}

#[program]
pub mod swap {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, admin: Pubkey, referral: Pubkey) -> Result<()> {
        require_keys_eq!(admin, ctx.accounts.admin.key(), SwapError::InvalidAdmin);
        require_keys_eq!(referral, ctx.accounts.referral.key(), SwapError::InvalidReferral);
        let swap_account = &mut ctx.accounts.swap_account;
        swap_account.admin = admin;
        swap_account.referral = referral;
        Ok(())
    }

    pub fn swap_tokens(
        ctx: Context<SwapTokens>,
        input_amount: u64,
        min_output_amount: u64,
        route_data: Vec<u8>,
    ) -> Result<()> {
        // Validate input and route
        require_gt!(input_amount, 0, SwapError::InvalidAmount);
        require_gt!(min_output_amount, 0, SwapError::InvalidMinOutAmount);
        require_keys_eq!(ctx.accounts.jupiter_program.key(), JUPITER_PROGRAM_ID, SwapError::InvalidJupiterRoute);
        require_keys_eq!(ctx.accounts.usdc_mint.key(), USDC_MINT, SwapError::InvalidJupiterRoute);

        // Commission calculation
        let commission = (input_amount as u128)
            .checked_mul(1)
            .and_then(|v| v.checked_div(100))
            .ok_or(SwapError::CommissionOverflow)? as u64;
        let amount_after = input_amount.checked_sub(commission).ok_or(SwapError::CommissionOverflow)?;

        // Transfer input tokens to program account
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.user_token_account.to_account_info(),
                    to: ctx.accounts.swap_token_account.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            input_amount,
        )?;

        // User swap via Jupiter CPI
        let swap_ix = Instruction {
            program_id: JUPITER_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(ctx.accounts.swap_token_account.key(), false),
                AccountMeta::new(ctx.accounts.user_sol_account.key(), false),
                AccountMeta::new(ctx.accounts.user.key(), true),
                AccountMeta::new(ctx.accounts.token_program.key(), false),
                AccountMeta::new(ctx.accounts.system_program.key(), false),
                AccountMeta::new_readonly(JUPITER_PROGRAM_ID, false),
                AccountMeta::new_readonly(ctx.accounts.jupiter_route.key(), false),
            ],
            data: route_data.clone(),
        };
        anchor_lang::solana_program::program::invoke(
            &swap_ix,
            &[
                ctx.accounts.swap_token_account.to_account_info(),
                ctx.accounts.user_sol_account.to_account_info(),
                ctx.accounts.user.to_account_info(),
                ctx.accounts.token_program.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
                ctx.accounts.jupiter_program.to_account_info(),
                ctx.accounts.jupiter_route.to_account_info(),
            ],
        )?;

        // Slippage check
        let sol_balance = ctx.accounts.user_sol_account.lamports();
        require_gte!(sol_balance, min_output_amount, SwapError::SlippageExceeded);

        // Commission swap via Jupiter CPI
        let commission_ix = Instruction {
            program_id: JUPITER_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(ctx.accounts.swap_token_account.key(), false),
                AccountMeta::new(ctx.accounts.commission_usdc_account.key(), false),
                AccountMeta::new(ctx.accounts.swap_account.key(), true),
                AccountMeta::new(ctx.accounts.token_program.key(), false),
                AccountMeta::new(ctx.accounts.system_program.key(), false),
                AccountMeta::new_readonly(JUPITER_PROGRAM_ID, false),
                AccountMeta::new_readonly(ctx.accounts.jupiter_route.key(), false),
            ],
            data: route_data,
        };
        anchor_lang::solana_program::program::invoke_signed(
            &commission_ix,
            &[
                ctx.accounts.swap_token_account.to_account_info(),
                ctx.accounts.commission_usdc_account.to_account_info(),
                ctx.accounts.swap_account.to_account_info(),
                ctx.accounts.token_program.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
                ctx.accounts.jupiter_program.to_account_info(),
                ctx.accounts.jupiter_route.to_account_info(),
            ],
            &[&[b"swap", &[ctx.bumps["swap_account"].clone()]]],
        )?;

        // Distribute USDC commission
        let usdc_amount = ctx.accounts.commission_usdc_account.amount;
        let referral_amt = (usdc_amount as u128).checked_mul(40).and_then(|v| v.checked_div(100)).ok_or(SwapError::CommissionOverflow)? as u64;
        let admin_amt = usdc_amount.checked_sub(referral_amt).ok_or(SwapError::CommissionOverflow)?;
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.commission_usdc_account.to_account_info(),
                    to: ctx.accounts.referral_usdc_account.to_account_info(),
                    authority: ctx.accounts.swap_account.to_account_info(),
                },
            ),
            referral_amt,
        )?;
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.commission_usdc_account.to_account_info(),
                    to: ctx.accounts.admin_usdc_account.to_account_info(),
                    authority: ctx.accounts.swap_account.to_account_info(),
                },
            ),
            admin_amt,
        )?;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init, payer = admin, space = 8 + 32 + 32, seeds = [b"swap"], bump)]
    pub swap_account: Account<'info, SwapState>,
    #[account(mut)] pub admin: Signer<'info>,
    #[account(mut)] pub referral: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SwapTokens<'info> {
    #[account(mut, seeds = [b"swap"], bump)] pub swap_account: Account<'info, SwapState>,
    #[account(mut)] pub user: Signer<'info>,
    #[account(mut)] pub input_token_mint: Account<'info, Mint>,
    #[account(mut, constraint = user_token_account.mint == input_token_mint.key(), constraint = user_token_account.owner == user.key())]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(mut, associated_token::mint = input_token_mint, associated_token::authority = swap_account)]
    pub swap_token_account: Account<'info, TokenAccount>,
    #[account(mut, associated_token::mint = USDC_MINT, associated_token::authority = swap_account)]
    pub commission_usdc_account: Account<'info, TokenAccount>,
    #[account(mut, associated_token::mint = USDC_MINT, associated_token::authority = referral.key())]
    pub referral_usdc_account: Account<'info, TokenAccount>,
    #[account(mut, associated_token::mint = USDC_MINT, associated_token::authority = admin.key())]
    pub admin_usdc_account: Account<'info, TokenAccount>,
    #[account(constraint = jupiter_program.key() == JUPITER_PROGRAM_ID)] pub jupiter_program: Program<'info, System>,
    pub jupiter_route: AccountInfo<'info>,
    #[account(mut)] pub user_sol_account: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub usdc_mint: Account<'info, Mint>,
}

#[account]
pub struct SwapState {
    pub admin: Pubkey,
    pub referral: Pubkey,
}
