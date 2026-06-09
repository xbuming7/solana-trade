#![allow(deprecated)]
use anchor_lang::{
    prelude::*,
    solana_program::{instruction::Instruction, program::invoke},
};
use anchor_spl::token_interface::TokenAccount;

declare_id!("onepE9wvoVHFEsmyyvaTrDWkXXmr7fd8Eu6k1RPnYQD");

#[program]
pub mod onep {
    use super::*;

    pub fn arb_route(ctx: Context<ArbRouteContext>, params: ArbRouteParams) -> Result<()> {
        let ArbRouteParams {
            buy_route,
            mut sell_route,
            amount_offset,
            min_quote_amount,
        } = params;

        let base_amount_before = ctx.accounts.base_token_account.amount;
        let quote_amount_before = ctx.accounts.quote_token_account.amount;

        let buy_account_count = buy_route.account_count as usize;
        let sell_account_count = sell_route.account_count as usize;

        // remaining_accounts: [buy_program, buy_accounts..., sell_program, sell_accounts...]
        let buy_program = &ctx.remaining_accounts[0];
        let buy_accounts = &ctx.remaining_accounts[1..=buy_account_count];
        let sell_program = &ctx.remaining_accounts[buy_account_count + 1];
        let sell_accounts = &ctx.remaining_accounts
            [buy_account_count + 2..buy_account_count + 2 + sell_account_count];

        // 执行买入
        invoke_dex_route(*buy_program.key, &buy_route.instruction_data, buy_accounts)?;

        // 用买入后的余额作为卖出输入
        ctx.accounts.base_token_account.reload()?;
        let base_amount_after = ctx.accounts.base_token_account.amount;
        // 安全减法：若买入后 base 代币余额减少（滑点/手续费等），直接报错
        let base_amount_changed = base_amount_after
            .checked_sub(base_amount_before)
            .ok_or(Errors::BaseAmountOverflow)?;

        patch_input_amount(
            &mut sell_route.instruction_data,
            amount_offset as usize,
            base_amount_changed,
        )?;

        // 执行卖出
        invoke_dex_route(
            *sell_program.key,
            &sell_route.instruction_data,
            sell_accounts,
        )?;

        ctx.accounts.quote_token_account.reload()?;
        let quote_amount_after = ctx.accounts.quote_token_account.amount;
        // 安全减法：若 quote_amount_after < quote_amount_before 说明亏损，直接判定利润校验失败
        let quote_amount_changed = quote_amount_after
            .checked_sub(quote_amount_before)
            .ok_or(Errors::ProfitCheckFailed)?;

        msg!(
            "Quote amount: {} -> {}, changed={}, min={}",
            quote_amount_before,
            quote_amount_after,
            quote_amount_changed,
            min_quote_amount
        );

        // 利润校验
        require_gte!(
            quote_amount_changed,
            min_quote_amount,
            Errors::ProfitCheckFailed
        );

        Ok(())
    }
}

#[derive(Accounts)]
pub struct ArbRouteContext<'info> {
    pub signer: Signer<'info>,
    pub quote_token_account: Box<InterfaceAccount<'info, TokenAccount>>,
    pub base_token_account: Box<InterfaceAccount<'info, TokenAccount>>,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct ArbRouteParams {
    pub buy_route: InstructionParams,
    pub sell_route: InstructionParams,
    pub amount_offset: u16,
    pub min_quote_amount: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct InstructionParams {
    pub account_count: u8,
    pub instruction_data: Vec<u8>,
}

#[inline]
fn patch_input_amount(data: &mut Vec<u8>, offset: usize, amount: u64) -> Result<()> {
    data[offset..offset + 8].copy_from_slice(&amount.to_le_bytes());
    Ok(())
}

#[inline]
fn invoke_dex_route(
    program_id: Pubkey,
    instruction_data: &[u8],
    accounts: &[AccountInfo<'_>],
) -> Result<()> {
    let mut account_metas = Vec::with_capacity(accounts.len());
    for acc in accounts {
        account_metas.push(AccountMeta {
            pubkey: *acc.key,
            is_signer: acc.is_signer,
            is_writable: acc.is_writable,
        });
    }

    invoke(
        &Instruction {
            program_id,
            accounts: account_metas,
            data: instruction_data.to_vec(),
        },
        accounts,
    )?;
    Ok(())
}

#[error_code]
pub enum Errors {
    #[msg("Profit check failed")]
    ProfitCheckFailed,
    #[msg("Base amount overflow")]
    BaseAmountOverflow,
}
