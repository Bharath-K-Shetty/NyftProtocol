use anchor_lang::prelude::*;
use anchor_lang::system_program;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{transfer, Mint, Token, TokenAccount, Transfer},
};
use ephemeral_rollups_sdk::anchor::{delegate, ephemeral};
use ephemeral_rollups_sdk::cpi::DelegateConfig;

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

#[ephemeral]
#[program]
pub mod nyft_trade {
    use super::*;

    /// Step 1: Initialize the escrow (PDA for each order)
    pub fn initialize_escrow(ctx: Context<InitializeEscrow>, order_id: u64) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow_account;
        escrow.owner = ctx.accounts.user.key();
        escrow.balance = 0;
        escrow.order_id = order_id;
        escrow.limit_order = LimitOrder::default();
        msg!(
            "Initialized escrow for user {} with order_id {}",
            escrow.owner,
            order_id
        );
        Ok(())
    }

    /// Step 2: Delegate the PDA to the ephemeral rollup
    pub fn delegate_order(ctx: Context<DelegateOrder>, order_id: u64) -> Result<()> {
        ctx.accounts.delegate_pda(
            &ctx.accounts.user,
            &[
                b"escrow",
                ctx.accounts.user.key().as_ref(),
                &order_id.to_le_bytes(),
            ],
            DelegateConfig::default(),
        )?;

        msg!(
            "Delegated escrow PDA for order_id {} to ephemeral validator",
            order_id
        );
        Ok(())
    }

    pub fn deposit_sol(ctx: Context<DepositSol>, amount: u64) -> Result<()> {
        let transfer_instruction = system_program::Transfer {
            from: ctx.accounts.user.to_account_info(),
            to: ctx.accounts.escrow_account.to_account_info(),
        };
        let cpi_context = CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            transfer_instruction,
        );
        system_program::transfer(cpi_context, amount)?;

        ctx.accounts.escrow_account.balance = ctx
            .accounts
            .escrow_account
            .balance
            .checked_add(amount)
            .unwrap();

        msg!(
            "Deposited {} lamports for BUY order {}",
            amount,
            ctx.accounts.escrow_account.order_id
        );
        Ok(())
    }

    pub fn deposit_tokens(ctx: Context<DepositTokens>, amount: u64) -> Result<()> {
        msg!("Transferring tokens...");
        msg!("Mint: {}", &ctx.accounts.token_mint.to_account_info().key());

        // let cpi_ctx = CpiContext::new(
        //     ctx.accounts.token_program.to_account_info(),
        //     token::Transfer {
        //         from: ctx.accounts.user_token_account.to_account_info(),
        //         to: ctx.accounts.escrow_token_account.to_account_info(),
        //         authority: ctx.accounts.user.to_account_info(),
        //     },
        // );
        // token::transfer(cpi_ctx, amount)?;

        // ctx.accounts.escrow_account.balance = ctx
        //     .accounts
        //     .escrow_account
        //     .balance
        //     .checked_add(amount)
        //     .unwrap();
        transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.user_token_account.to_account_info(),
                    to: ctx.accounts.escrow_token_account.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            amount * 10u64.pow(ctx.accounts.token_mint.decimals as u32),
        );
        msg!(
            "Deposited {} tokens of mint {} for SELL order {}",
            amount,
            ctx.accounts.token_mint.key(),
            ctx.accounts.escrow_account.order_id
        );

        Ok(())
    }
    /// Step 4: Create a limit order (buy/sell)
    pub fn create_limit_order(
        ctx: Context<UpdateEscrow>,
        order_type: OrderType,
        token_mint: Pubkey,
        limit_price: u64,
    ) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow_account;
        if order_type == OrderType::Buy {
            require!(
                escrow.balance >= limit_price,
                CustomError::InsufficientFunds
            );
        }

        escrow.limit_order = LimitOrder {
            is_active: true,
            order_type,
            token_mint,
            limit_price,
        };

        msg!(
            "Created {:?} order for token {} at price {} (order_id: {})",
            order_type,
            token_mint,
            limit_price,
            escrow.order_id
        );
        Ok(())
    }

    /// Step 5: Cancel limit order
    pub fn cancel_limit_order(ctx: Context<UpdateEscrow>) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow_account;
        escrow.limit_order.is_active = false;
        msg!("Cancelled order_id {}", escrow.order_id);
        Ok(())
    }

    /// Step 6: Execute limit order (triggered by crank)
    pub fn execute_limit_order(ctx: Context<ExecuteOrder>) -> Result<()> {
        let escrow = &ctx.accounts.escrow_account;
        require!(escrow.limit_order.is_active, CustomError::OrderNotActive);

        msg!(
            "Executed order_id {} for owner {}",
            escrow.order_id,
            escrow.owner
        );
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(order_id: u64)]
pub struct InitializeEscrow<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        init,
        payer = user,
        space = 8 + EscrowAccount::SIZE,
        seeds = [b"escrow", user.key().as_ref(), &order_id.to_le_bytes()],
        bump
    )]
    pub escrow_account: Account<'info, EscrowAccount>,

    pub system_program: Program<'info, System>,
}
#[derive(Accounts)]
pub struct DepositSol<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(mut)]
    pub escrow_account: Account<'info, EscrowAccount>,

    pub system_program: Program<'info, System>,
}
#[derive(Accounts)]
pub struct DepositTokens<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    pub token_mint: Account<'info, Mint>,

    #[account(mut)]
    pub escrow_account: Account<'info, EscrowAccount>,

    #[account(mut,
       associated_token::mint = token_mint,
        associated_token::authority = user)]
    pub user_token_account: Account<'info, TokenAccount>,

    #[account(init_if_needed,payer=user,associated_token::mint=token_mint,
    associated_token::authority=escrow_account)]
    pub escrow_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

/// Delegate PDA to rollup validator
#[delegate]
#[derive(Accounts)]
pub struct DelegateOrder<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    /// The escrow PDA to delegate
    #[account(mut, del, seeds = [b"escrow", user.key().as_ref(), &order_id.to_le_bytes()], bump)]
    pub escrow_account: Account<'info, EscrowAccount>,
}

#[derive(Accounts)]
pub struct UpdateEscrow<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(
        mut,
        constraint = escrow_account.owner == user.key() @ CustomError::Unauthorized
    )]
    pub escrow_account: Account<'info, EscrowAccount>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ExecuteOrder<'info> {
    pub crank: Signer<'info>,
    #[account(mut)]
    pub escrow_account: Account<'info, EscrowAccount>,
    pub owner: AccountInfo<'info>,
}

#[account]
pub struct EscrowAccount {
    pub owner: Pubkey,
    pub balance: u64,
    pub order_id: u64,
    pub limit_order: LimitOrder,
}

impl EscrowAccount {
    pub const SIZE: usize = 32 + 8 + 8 + LimitOrder::SIZE;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default, PartialEq)]
pub struct LimitOrder {
    pub is_active: bool,
    pub order_type: OrderType,
    pub token_mint: Pubkey,
    pub limit_price: u64,
}

impl LimitOrder {
    pub const SIZE: usize = 1 + 1 + 32 + 8;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq)]
pub enum OrderType {
    Buy,
    Sell,
}

impl Default for OrderType {
    fn default() -> Self {
        OrderType::Buy
    }
}

#[error_code]
pub enum CustomError {
    #[msg("Insufficient funds to perform this action.")]
    InsufficientFunds,
    #[msg("The order is not active and cannot be executed.")]
    OrderNotActive,
    #[msg("You are not authorized to perform this action.")]
    Unauthorized,
}
