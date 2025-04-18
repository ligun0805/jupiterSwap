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

/// Main program module for the swap functionality

#[program]
pub mod swap {
    use super::*;

    /// Initialize the swap program with admin and referral addresses
    /// 
    /// # Arguments
    /// * `ctx` - The context of accounts
    /// * `admin` - The admin wallet address that will receive 0.6% commission
    /// * `referral` - The referral wallet address that will receive 0.4% commission
    pub fn initialize(ctx: Context<Initialize>, admin: Pubkey, referral: Pubkey) -> Result<()> {
        require_keys_eq!(admin, ctx.accounts.admin.key(), SwapError::InvalidAdmin);
        require_keys_eq!(referral, ctx.accounts.referral.key(), SwapError::InvalidReferral);
        
        let swap_account = &mut ctx.accounts.swap_account;
        swap_account.admin = admin;
        swap_account.referral = referral;
        Ok(())
    }

    /// Execute a token swap with commission handling
    /// 
    /// # Arguments
    /// * `ctx` - The context of accounts
    /// * `input_amount` - Amount of input tokens to swap
    /// * `min_output_amount` - Minimum amount of SOL to receive after swap
    /// * `route_data` - Jupiter route data for the swap
    /// 
    /// # Flow
    /// 1. Calculate 1% commission from input amount
    /// 2. Split commission into referral (0.4%) and admin (0.6%) portions
    /// 3. Execute main token swap through Jupiter for user
    /// 4. Verify minimum output amount
    /// 5. Execute Jupiter swap for commission tokens to USDC
    /// 6. Distribute USDC to referral and admin wallets
    /// 
    /// # Example
    /// For 5870 WIF tokens:
    /// - Commission: 58.7 WIF (1%)
    /// - User swap: 5811.3 WIF → 57.4774 SOL
    /// - Commission conversion: 58.7 WIF → 70.6465 USDC
    /// - Referral receives: 28.2586 USDC
    /// - Admin receives: 42.3879 USDC
    pub fn swap_tokens(
        ctx: Context<SwapTokens>,
        input_amount: u64,
        min_output_amount: u64,
        route_data: Vec<u8>,
    ) -> Result<()> {
        // Validate input amount
        require_gt!(input_amount, 0, SwapError::InvalidAmount);
        require_gt!(min_output_amount, 0, SwapError::InvalidMinOutAmount);

        // Validate Jupiter program ID
        require_keys_eq!(
            ctx.accounts.jupiter_program.key(),
            JUPITER_PROGRAM_ID,
            SwapError::InvalidJupiterRoute
        );

        // Validate USDC mint
        require_keys_eq!(
            ctx.accounts.usdc_mint.key(),
            USDC_MINT,
            SwapError::InvalidJupiterRoute
        );

        let _swap_account = &ctx.accounts.swap_account;
        
        // Calculate commission with overflow protection
        let commission = (input_amount as u128)
            .checked_mul(1u128)
            .and_then(|v| v.checked_div(100u128))
            .ok_or(SwapError::CommissionOverflow)? as u64;

        let _amount_after_commission = input_amount.checked_sub(commission)
            .ok_or(SwapError::CommissionOverflow)?;

        // Check user's token balance
        let user_balance = ctx.accounts.user_token_account.amount;
        require_gte!(user_balance, input_amount, SwapError::InsufficientBalance);

        // Transfer tokens in a single CPI call
        let _transfer_ix = token::transfer(
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

        // Execute Jupiter swap for user's tokens
        let jupiter_swap_ix = anchor_lang::solana_program::instruction::Instruction {
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
            &jupiter_swap_ix,
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

        // Verify minimum output amount with slippage protection
        let user_sol_balance = ctx.accounts.user_sol_account.lamports();
        require_gte!(user_sol_balance, min_output_amount, SwapError::SlippageExceeded);

        // Execute Jupiter swap for commission tokens to USDC
        let jupiter_commission_ix = anchor_lang::solana_program::instruction::Instruction {
            program_id: JUPITER_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(ctx.accounts.commission_token_account.key(), false),
                AccountMeta::new(ctx.accounts.commission_usdc_account.key(), false),
                AccountMeta::new(ctx.accounts.swap_account.key(), true),
                AccountMeta::new(ctx.accounts.token_program.key(), false),
                AccountMeta::new(ctx.accounts.system_program.key(), false),
                AccountMeta::new_readonly(JUPITER_PROGRAM_ID, false),
                AccountMeta::new_readonly(ctx.accounts.jupiter_route.key(), false),
            ],
            data: route_data,
        };

        anchor_lang::solana_program::program::invoke(
            &jupiter_commission_ix,
            &[
                ctx.accounts.commission_token_account.to_account_info(),
                ctx.accounts.commission_usdc_account.to_account_info(),
                ctx.accounts.swap_account.to_account_info(),
                ctx.accounts.token_program.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
                ctx.accounts.jupiter_program.to_account_info(),
                ctx.accounts.jupiter_route.to_account_info(),
            ],
        )?;

        // Get the actual USDC amount received from the swap
        let usdc_balance = ctx.accounts.commission_usdc_account.amount;
        
        // Calculate referral and admin shares from actual USDC amount
        let referral_usdc = (usdc_balance as u128)
            .checked_mul(40u128)
            .and_then(|v| v.checked_div(100u128))
            .ok_or(SwapError::CommissionOverflow)? as u64;

        let admin_usdc = usdc_balance.checked_sub(referral_usdc)
            .ok_or(SwapError::CommissionOverflow)?;

        // Transfer USDC to referral and admin wallets
        let _referral_transfer = token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.commission_usdc_account.to_account_info(),
                    to: ctx.accounts.referral_usdc_account.to_account_info(),
                    authority: ctx.accounts.swap_account.to_account_info(),
                },
            ),
            referral_usdc,
        )?;

        let _admin_transfer = token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.commission_usdc_account.to_account_info(),
                    to: ctx.accounts.admin_usdc_account.to_account_info(),
                    authority: ctx.accounts.swap_account.to_account_info(),
                },
            ),
            admin_usdc,
        )?;

        Ok(())
    }
}

/// Accounts required for program initialization
#[derive(Accounts)]
pub struct Initialize<'info> {
    /// The program's state account
    #[account(
        init,
        payer = admin,
        space = 8 + 32 + 32, // discriminator + admin pubkey + referral pubkey
        seeds = [b"swap".as_ref()],
        bump
    )]
    pub swap_account: Account<'info, SwapAccount>,
    
    /// The admin who will pay for initialization
    #[account(mut)]
    pub admin: Signer<'info>,
    
    /// The referral wallet address
    #[account(mut)]
    pub referral: Signer<'info>,
    
    /// Required for account initialization
    pub system_program: Program<'info, System>,
}

/// Accounts required for token swaps
#[derive(Accounts)]
pub struct SwapTokens<'info> {
    /// The program's state account
    #[account(
        mut,
        seeds = [b"swap".as_ref()],
        bump
    )]
    pub swap_account: Account<'info, SwapAccount>,
    
    /// The user performing the swap
    #[account(mut)]
    pub user: Signer<'info>,
    
    /// The mint of the input token
    #[account(mut)]
    pub input_token_mint: Account<'info, Mint>,
    
    /// The user's token account for the input token
    #[account(
        mut,
        constraint = user_token_account.mint == input_token_mint.key(),
        constraint = user_token_account.owner == user.key(),
    )]
    pub user_token_account: Account<'info, TokenAccount>,
    
    /// The program's token account for handling swaps
    #[account(
        mut,
        associated_token::mint = input_token_mint,
        associated_token::authority = swap_account
    )]
    pub swap_token_account: Account<'info, TokenAccount>,
    
    /// The program's token account for commission
    #[account(
        mut,
        associated_token::mint = input_token_mint,
        associated_token::authority = swap_account
    )]
    pub commission_token_account: Account<'info, TokenAccount>,
    
    /// The program's USDC account for converted commission
    #[account(
        mut,
        associated_token::mint = usdc_mint,
        associated_token::authority = swap_account
    )]
    pub commission_usdc_account: Account<'info, TokenAccount>,
    
    /// The referral wallet's USDC account
    #[account(
        mut,
        associated_token::mint = usdc_mint,
        associated_token::authority = swap_account.referral
    )]
    pub referral_usdc_account: Account<'info, TokenAccount>,
    
    /// The admin wallet's USDC account
    #[account(
        mut,
        associated_token::mint = usdc_mint,
        associated_token::authority = swap_account.admin
    )]
    pub admin_usdc_account: Account<'info, TokenAccount>,
    
    /// The USDC token mint
    #[account(
        constraint = usdc_mint.key() == USDC_MINT
    )]
    pub usdc_mint: Account<'info, Mint>,
    
    /// SPL Token program
    pub token_program: Program<'info, Token>,
    
    /// Associated Token program
    pub associated_token_program: Program<'info, AssociatedToken>,
    
    /// System program
    pub system_program: Program<'info, System>,
    
    /// Jupiter program
    #[account(
        constraint = jupiter_program.key() == JUPITER_PROGRAM_ID
    )]
    pub jupiter_program: Program<'info, System>,
    
    /// Jupiter route
    /// CHECK: This is a Jupiter route account that will be validated by the Jupiter program
    pub jupiter_route: AccountInfo<'info>,
    
    /// User's SOL account
    /// CHECK: This is the user's SOL account needed for paying network fees
    #[account(mut)]
    pub user_sol_account: AccountInfo<'info>,
}

/// The program's state account structure
#[account]
pub struct SwapAccount {
    /// The admin wallet address that receives 0.6% commission
    pub admin: Pubkey,
    /// The referral wallet address that receives 0.4% commission
    pub referral: Pubkey,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Overflow occurred")]
    Overflow,
}
