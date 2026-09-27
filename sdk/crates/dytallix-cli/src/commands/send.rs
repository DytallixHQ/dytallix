//! `dytallix send` on the consensus chain: one ordinary-v2 transfer signed
//! against the pinned chain (clients v1, K-c). A transfer to an address with
//! no account creates it and burns the chain's account creation fee.
use anyhow::{ensure, Result};
use clap::{Args, ValueEnum};
use dytallix_sdk::ordinary_v2::{parse_token_units, Action, Denomination};

use super::consensus::{self, Connection, WriteArgs};
use super::ordinary::print_json;

#[derive(Debug, Clone, Args)]
pub struct SendArgs {
    /// Recipient account address on the pinned network.
    #[arg(long)]
    to: String,
    /// Amount in tokens, with up to six decimal places.
    #[arg(long)]
    amount: String,
    #[arg(long, value_enum, default_value = "drt")]
    token: Token,
    #[command(flatten)]
    connection: Connection,
    #[command(flatten)]
    write: WriteArgs,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Token {
    Dgt,
    Drt,
}
impl From<Token> for Denomination {
    fn from(token: Token) -> Self {
        match token {
            Token::Dgt => Denomination::Udgt,
            Token::Drt => Denomination::Udrt,
        }
    }
}

pub async fn run(args: SendArgs) -> Result<()> {
    let session = args.connection.open()?;
    let amount = parse_token_units(&args.amount)?;
    ensure!(amount > 0, "amount must be positive");
    let action = Action::Send {
        recipient: consensus::recipient(&session.pin, &args.to)?,
        denomination: args.token.into(),
        amount,
    };
    print_json(&consensus::submit_ordinary(&session, vec![action], args.write).await?)
}
