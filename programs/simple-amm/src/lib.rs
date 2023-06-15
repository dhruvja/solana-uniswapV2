use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token,TokenAccount, Transfer, MintTo},
};

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

const LP_TOKEN_SEED: &'static [u8] = b"liquidity_token";
const AMM_STATE_SEED: &'static [u8] = b"amm_state";
const POOL_SEED: &'static [u8] = b"pool";

pub fn quote(amount_a: u64, token_a_reserves: u64, token_b_reserves: u64) -> Result<u64> {
    require!(amount_a > 0, ErrorCodes::InsufficientAmountToDeposit);
    require!(token_a_reserves > 0 && token_b_reserves > 0, ErrorCodes::InsufficientLiquidity);
    let amount_b = (amount_a * token_b_reserves)/token_a_reserves;
    Ok(amount_b)
}

pub fn get_quote(token_a_reserves: u64, token_b_reserves: u64, amount_a_desired: u64, amount_b_desired: u64, amount_a_min: u64, amount_b_min: u64) -> Result<(u64, u64)> {
    if token_a_reserves == 0 && token_b_reserves == 0 {
        Ok((amount_a_desired, amount_b_desired))
    } else {
        let amount_b_optimal = quote(amount_a_desired, token_a_reserves, token_b_reserves).unwrap();
        if amount_b_optimal <= amount_b_desired {
            require!(amount_b_optimal > amount_b_min, ErrorCodes::InsufficientTokenBAmount);
            Ok((amount_a_desired, amount_b_optimal))
        } else {
            let amount_a_optimal = quote(amount_b_desired, token_b_reserves, token_a_reserves).unwrap(); 
            assert!(amount_a_optimal <= amount_a_desired);
            require!(amount_a_optimal > amount_a_min, ErrorCodes::InsufficientTokenAAmount);
            Ok((amount_a_optimal, amount_b_desired)) 
        }
    }
}

#[program]
pub mod simple_amm {
    use super::*;

    // Initialize AMM and set authority and LP fees
    pub fn initialize_amm(ctx: Context<Initialize>, lp_fees_basis_points: u8) -> Result<()> {
        let parameters = &mut ctx.accounts.amm_state;

        parameters.authority = ctx.accounts.authority.key();
        parameters.lp_fees_basis_points = lp_fees_basis_points;

        msg!(
            "AMM is initialized with {} as authority and {} basis points",
            parameters.authority,
            parameters.lp_fees_basis_points
        );
        emit!(InitializeAMM {
            authority: parameters.authority,
            lp_fees_basis_points: parameters.lp_fees_basis_points
        });

        Ok(())
    }

    // Creates liquidity
    pub fn add_liquidity(ctx: Context<AddLiquidity>, lp_token_bump: u8, amount_a_desired: u64, amount_b_desired: u64, amount_a_min: u64, amount_b_min: u64) -> Result<()> {
        msg!("Called successfully");

        let lp_token_mint = &ctx.accounts.liquidity_token_mint;
        let token_a_mint = ctx.accounts.token_a_mint.key();
        let token_b_mint = ctx.accounts.token_b_mint.key();
        let token_a_pool = &ctx.accounts.token_a_pool;
        let token_b_pool = &ctx.accounts.token_b_pool;
        let token_a_account = &ctx.accounts.token_a_account;
        let token_b_account = &ctx.accounts.token_b_account;

        // SEEDS for PDA for CPI call
        let bump_vector = lp_token_bump.to_le_bytes();
        let inner = vec![
            LP_TOKEN_SEED,
            token_a_mint.as_ref(),
            token_b_mint.as_ref(),
            bump_vector.as_ref(),
        ];
        let outer = vec![inner.as_slice()];


        // Get reserves
        let token_a_reserves = token_a_pool.amount;
        let token_b_reserves = token_b_pool.amount;
        // Get Quote
        let (amount_a, amount_b) = get_quote(token_a_reserves, token_b_reserves, amount_a_desired, amount_b_desired, amount_a_min, amount_b_min).unwrap();
        // Deposit the tokens to the pool
        let transfer_instruction_a = Transfer {
            from: token_a_account.to_account_info(),
            to: ctx.accounts.token_a_pool.to_account_info(),
            authority: ctx.accounts.liquidity_provider.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_instruction_a,
            outer.as_slice(), //signer PDA
        );
        anchor_spl::token::transfer(cpi_ctx, amount_a)?; 

        let transfer_instruction_b = Transfer {
            from: token_b_account.to_account_info(),
            to: ctx.accounts.token_b_pool.to_account_info(),
            authority: ctx.accounts.liquidity_provider.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_instruction_b,
            outer.as_slice(), //signer PDA
        );
        anchor_spl::token::transfer(cpi_ctx, amount_b)?;
        // Calculate the liquidity tokens to mint
        let liquidity_tokens_to_mint = (f64::sqrt((amount_a * amount_b) as f64)).floor() as u64;
        msg!("Liquidity tokens to mint {}", liquidity_tokens_to_mint);
        // Mint the liquidity tokens
        let mint_instruction = MintTo {
            mint: lp_token_mint.to_account_info(),
            to: ctx.accounts.lp_token_account.to_account_info(),
            authority: lp_token_mint.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            mint_instruction,
            outer.as_slice(), //signer PDA
        );
        anchor_spl::token::mint_to(cpi_ctx, liquidity_tokens_to_mint)?;
        Ok(())
    }

    pub fn remove_liquidity(ctx: Context<RemoveLiquidity>) -> Result<()> {
        Ok(())
    }

    pub fn swap_token_for_token(ctx: Context<SwapToken>) -> Result<()> {
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(init, payer = authority, seeds = [AMM_STATE_SEED], bump, space = 8 + AmmState::INIT_SPACE)]
    pub amm_state: Account<'info, AmmState>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct AddLiquidity<'info> {
    #[account(mut)]
    pub liquidity_provider: Signer<'info>,
    #[account(mut, seeds = [AMM_STATE_SEED], bump)]
    pub amm_state: Account<'info, AmmState>,
    #[account(
        init_if_needed,
        payer = liquidity_provider,
        mint::decimals = 6,
        mint::authority = liquidity_token_mint,
        seeds=[LP_TOKEN_SEED, token_a_mint.key().as_ref() , token_b_mint.key().as_ref()],
        bump,
    )]
    pub liquidity_token_mint: Box<Account<'info, Mint>>,
    #[account(
        init_if_needed,
        payer = liquidity_provider,
        associated_token::mint = liquidity_token_mint, 
        associated_token::authority = liquidity_provider
    )]
    pub lp_token_account: Box<Account<'info, TokenAccount>>,
    pub token_a_mint: Box<Account<'info, Mint>>,
    pub token_b_mint: Box<Account<'info, Mint>>,
    #[account
    (
        mut, 
        constraint=token_a_account.owner == liquidity_provider.key(),
        constraint=token_a_account.mint == token_a_mint.key()
    )]
    pub token_a_account: Account<'info, TokenAccount>,
    #[account(
        mut, 
        constraint=token_b_account.owner == liquidity_provider.key(),
        constraint=token_b_account.mint == token_b_mint.key()
    )]
    pub token_b_account: Box<Account<'info, TokenAccount>>,
    #[account(init_if_needed, payer = liquidity_provider, seeds = [POOL_SEED, token_a_mint.key().as_ref() , token_b_mint.key().as_ref()], bump, token::mint = token_a_mint, token::authority = token_a_pool)]
    pub token_a_pool: Box<Account<'info, TokenAccount>>,
    #[account(init_if_needed, payer = liquidity_provider, seeds = [POOL_SEED, token_b_mint.key().as_ref() , token_a_mint.key().as_ref()], bump, token::mint = token_b_mint, token::authority = token_b_pool)]
    pub token_b_pool: Box<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct RemoveLiquidity {}

#[derive(Accounts)]
pub struct SwapToken {}

#[account]
#[derive(InitSpace)]
pub struct AmmState {
    pub authority: Pubkey,        // 32
    pub lp_fees_basis_points: u8, // 1
}

#[event]
pub struct InitializeAMM {
    authority: Pubkey,
    lp_fees_basis_points: u8,
}

#[error_code]
#[derive(Eq, PartialEq)]
pub enum ErrorCodes {
    #[msg("Amount to be deposit is insufficient")]
    InsufficientAmountToDeposit,
    #[msg("Insufficient liquidity")]
    InsufficientLiquidity,
    #[msg("Insufficient token B amount")]
    InsufficientTokenBAmount,
    #[msg("Insufficient token A amount")]
    InsufficientTokenAAmount
}