//! Governance on the consensus chain: ordinary-v3 transactions (clients v1,
//! E04 gap 8). `propose`, `deposit` and `vote` run in one step through the
//! pinned chain's node (K-c). For offline signing, `prepare` builds a body
//! from captured views, `sign` signs it and `submit` sends it after
//! refreshing the views. An admitted transaction whose governance rule fails
//! is still charged.
use std::path::{Path, PathBuf};

use anyhow::{anyhow, ensure, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use dytallix_sdk::ordinary_v2::{
    parse_token_units, AccountAddress, AccountView, AddressNetwork, FeeQuote, KeypairSigner,
    ProfileView, SigningContext,
};
use dytallix_sdk::ordinary_v3::{
    self as governance, Action, FeeProfileV3, FeeValues, GovernanceProfileView,
    OrdinaryTransaction, ParameterChange, PreparedGovernance, RegistryChange, SignedOrdinary,
    VoteChoice,
};
use serde_json::{json, Value};

use super::bytes_to_hex;
use super::consensus::{self, Connection, WriteArgs};
use super::ordinary::{
    client, decimal_u128, decimal_u64, load_signing_key, print_json, read_json, write_json,
    CapturedInputs,
};

#[derive(Debug, Clone, Args)]
pub struct GovernanceArgs {
    #[command(subcommand)]
    pub command: GovernanceCommand,
}

#[derive(Debug, Clone, Args)]
pub struct GovernanceInputs {
    #[command(flatten)]
    ordinary: CapturedInputs,
    /// Captured committed governance profile query JSON (`query-profile`).
    #[arg(long)]
    governance_profile: PathBuf,
}

/// Fields every prepared governance body carries.
#[derive(Debug, Clone, Args)]
pub struct BodyArgs {
    #[arg(long, default_value = "")]
    memo: String,
    #[arg(long, value_parser = decimal_u64)]
    expiry_height: u64,
    #[arg(long, value_parser = decimal_u64)]
    gas_limit: u64,
    #[arg(long, value_parser = decimal_u128)]
    maximum_fee_udrt: u128,
    #[arg(long)]
    output: PathBuf,
}

/// The proposed change: exactly one.
#[derive(Debug, Clone, Args)]
#[group(required = true, multiple = false)]
pub struct ChangeArgs {
    /// Parameter change: the active validator set size.
    #[arg(long, value_parser = decimal_u64)]
    max_active: Option<u64>,
    /// Parameter change: the minimum validator self-bond in base units.
    #[arg(long, value_parser = decimal_u128)]
    min_self_bond_udgt: Option<u128>,
    /// Parameter change: every governed fee value, as a FeeValues JSON file.
    #[arg(long)]
    fees: Option<PathBuf>,
    /// Validator registry: approve this validator ID for --owner.
    #[arg(long, requires = "owner")]
    registry_add: Option<String>,
    /// Validator registry: withdraw this validator ID's approval.
    #[arg(long)]
    registry_remove: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Choice {
    Yes,
    No,
    NoWithVeto,
    Abstain,
}
impl From<Choice> for VoteChoice {
    fn from(choice: Choice) -> Self {
        match choice {
            Choice::Yes => VoteChoice::Yes,
            Choice::No => VoteChoice::No,
            Choice::NoWithVeto => VoteChoice::NoWithVeto,
            Choice::Abstain => VoteChoice::Abstain,
        }
    }
}

#[derive(Debug, Clone, Subcommand)]
pub enum GovernanceCommand {
    /// Propose one change in one step. It takes the node's next proposal
    /// ID; if another proposal commits first, this one fails and is charged.
    Propose {
        #[command(flatten)]
        change: ChangeArgs,
        /// Owner's account address, for --registry-add. It must be registered.
        #[arg(long, requires = "registry_add")]
        owner: Option<String>,
        #[command(flatten)]
        connection: Connection,
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Deposit DGT on a proposal in one step.
    Deposit {
        #[arg(long, value_parser = decimal_u64)]
        proposal_id: u64,
        /// DGT, with up to six decimal places.
        #[arg(long)]
        amount: String,
        #[command(flatten)]
        connection: Connection,
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Vote on a proposal in one step.
    Vote {
        #[arg(long, value_parser = decimal_u64)]
        proposal_id: u64,
        #[arg(long, value_enum)]
        choice: Choice,
        #[command(flatten)]
        connection: Connection,
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Show a proposal, and one account's vote with --voter.
    Show {
        #[arg(long, value_parser = decimal_u64)]
        proposal_id: u64,
        /// Account address whose vote to show.
        #[arg(long)]
        voter: Option<String>,
        /// Comet RPC endpoint; defaults to the pinned chain's endpoint.
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Prepare an unsigned body offline from captured views.
    Prepare(PrepareArgs),
    /// Read the committed governance fee profile and next proposal ID.
    QueryProfile {
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Sign an exact body offline with an explicitly selected existing key.
    Sign {
        #[command(flatten)]
        inputs: GovernanceInputs,
        #[arg(long)]
        body: PathBuf,
        /// Existing named keystore entry. Its legacy address is not used.
        #[arg(
            long,
            required_unless_present = "key_file",
            conflicts_with = "key_file"
        )]
        wallet: Option<String>,
        /// Private JSON file with algorithm, public_key, and private_key byte arrays.
        #[arg(long, required_unless_present = "wallet", conflicts_with = "wallet")]
        key_file: Option<PathBuf>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Validate a body or signature against captured state and display public fields.
    Inspect {
        #[command(flatten)]
        inputs: GovernanceInputs,
        #[arg(long, required_unless_present = "signed", conflicts_with = "signed")]
        body: Option<PathBuf>,
        #[arg(long, required_unless_present = "body", conflicts_with = "body")]
        signed: Option<PathBuf>,
    },
    /// Refresh the views, then submit an exact signed transaction. CheckTx
    /// acceptance is not commitment; the spent nonce is.
    Submit {
        #[command(flatten)]
        inputs: GovernanceInputs,
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        signed: PathBuf,
    },
}

#[derive(Debug, Clone, Args)]
pub struct PrepareArgs {
    #[command(subcommand)]
    pub command: PrepareCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum PrepareCommand {
    /// Prepare an unsigned proposal offline. It takes the captured next
    /// proposal ID; if another proposal commits first, this one fails and is
    /// charged, so submit it promptly.
    Propose {
        #[command(flatten)]
        inputs: GovernanceInputs,
        #[command(flatten)]
        change: ChangeArgs,
        /// Owner's account address, for --registry-add. It must be registered.
        #[arg(long, requires = "registry_add")]
        owner: Option<String>,
        #[command(flatten)]
        body: BodyArgs,
    },
    /// Prepare an unsigned deposit offline.
    Deposit {
        #[command(flatten)]
        inputs: GovernanceInputs,
        #[arg(long, value_parser = decimal_u64)]
        proposal_id: u64,
        #[arg(long, value_parser = decimal_u128)]
        amount_udgt: u128,
        #[command(flatten)]
        body: BodyArgs,
    },
    /// Prepare an unsigned vote offline.
    Vote {
        #[command(flatten)]
        inputs: GovernanceInputs,
        #[arg(long, value_parser = decimal_u64)]
        proposal_id: u64,
        #[arg(long, value_enum)]
        choice: Choice,
        #[command(flatten)]
        body: BodyArgs,
    },
}

struct Validated {
    profile: FeeProfileV3,
    view: ProfileView,
    governance: GovernanceProfileView,
    context: SigningContext,
}
impl GovernanceInputs {
    fn load(&self) -> Result<Validated> {
        let view: ProfileView = read_json(&self.ordinary.profile, false)?;
        let account: AccountView = read_json(&self.ordinary.account, false)?;
        let context: SigningContext = read_json(&self.ordinary.context, false)?;
        let governance_view: GovernanceProfileView = read_json(&self.governance_profile, false)?;
        let profile = governance::validate_views(&context, &view, &governance_view, &account)?;
        Ok(Validated {
            profile,
            view,
            governance: governance_view,
            context,
        })
    }
}
impl Validated {
    fn prepare(&self, action: Action, body: BodyArgs) -> Result<()> {
        let prepared = governance::prepare(
            &self.profile,
            &self.context,
            action,
            body.memo,
            body.expiry_height,
            body.gas_limit,
            body.maximum_fee_udrt,
        )?;
        write_json(&body.output, prepared.body())?;
        print_json(&describe(
            prepared.body(),
            &self.profile,
            "prepared_offline",
        )?)
    }
    fn body(&self, body: OrdinaryTransaction) -> Result<PreparedGovernance> {
        Ok(PreparedGovernance::from_body(
            body,
            &self.profile,
            &self.context,
        )?)
    }
    fn signed(&self, path: &Path) -> Result<SignedOrdinary> {
        let signed: SignedOrdinary = read_json(path, false)?;
        self.body(signed.body.clone())?;
        governance::verify_signature(&signed, &self.profile.limits())?;
        Ok(signed)
    }
    fn transport_bound(&self) -> Result<usize> {
        let config = self
            .view
            .config
            .as_ref()
            .context("ordinary configuration is missing")?;
        Ok(usize::try_from(config.max_transport_bytes)?)
    }
}

pub async fn run(args: GovernanceArgs) -> Result<()> {
    match args.command {
        GovernanceCommand::Propose {
            change,
            owner,
            connection,
            write,
        } => {
            let session = connection.open()?;
            let network = session.pin.network;
            let report = consensus::submit_governance(
                &session,
                |id| proposal(id, network, change, owner),
                write,
            )
            .await?;
            print_json(&report)?;
        }
        GovernanceCommand::Deposit {
            proposal_id,
            amount,
            connection,
            write,
        } => {
            let amount = parse_token_units(&amount)?;
            ensure!(amount > 0, "amount must be positive");
            let session = connection.open()?;
            let report = consensus::submit_governance(
                &session,
                |_| Ok(governance::deposit(proposal_id, amount)),
                write,
            )
            .await?;
            print_json(&report)?;
        }
        GovernanceCommand::Vote {
            proposal_id,
            choice,
            connection,
            write,
        } => {
            let session = connection.open()?;
            let report = consensus::submit_governance(
                &session,
                |_| Ok(governance::vote(proposal_id, choice.into())),
                write,
            )
            .await?;
            print_json(&report)?;
        }
        GovernanceCommand::Show {
            proposal_id,
            voter,
            endpoint,
        } => {
            let (pin, client) = consensus::open_chain(endpoint.as_deref())?;
            let proposal = client
                .query_proposal(&pin, proposal_id)
                .await?
                .with_context(|| format!("proposal {proposal_id} does not exist"))?;
            let vote = match voter {
                Some(raw) => {
                    let address = AccountAddress::decode(pin.network, &raw)
                        .map_err(|_| anyhow!("voter is not an address on the pinned network"))?;
                    client
                        .query_vote(&pin, proposal_id, address.account_id())
                        .await?
                }
                None => None,
            };
            print_json(&json!({"proposal": proposal, "vote": vote}))?;
        }
        GovernanceCommand::Prepare(PrepareArgs { command }) => match command {
            PrepareCommand::Propose {
                inputs,
                change,
                owner,
                body,
            } => {
                let inputs = inputs.load()?;
                let network = network(inputs.context.domain.network)?;
                let action = proposal(inputs.governance.next_proposal_id, network, change, owner)?;
                inputs.prepare(action, body)?;
            }
            PrepareCommand::Deposit {
                inputs,
                proposal_id,
                amount_udgt,
                body,
            } => {
                let inputs = inputs.load()?;
                inputs.prepare(governance::deposit(proposal_id, amount_udgt), body)?;
            }
            PrepareCommand::Vote {
                inputs,
                proposal_id,
                choice,
                body,
            } => {
                let inputs = inputs.load()?;
                inputs.prepare(governance::vote(proposal_id, choice.into()), body)?;
            }
        },
        GovernanceCommand::QueryProfile { endpoint, output } => {
            let value = client(&endpoint)?.query_governance_profile().await?;
            write_json(&output, &value)?;
            print_json(&value)?;
        }
        GovernanceCommand::Sign {
            inputs,
            body,
            wallet,
            key_file,
            output,
        } => {
            let inputs = inputs.load()?;
            let prepared = inputs.body(read_json(&body, false)?)?;
            let key = load_signing_key(wallet.as_deref(), key_file.as_deref())?;
            let signed = prepared.sign(&KeypairSigner::new(&key)?)?;
            write_json(&output, &signed)?;
            print_json(&describe(&signed.body, &inputs.profile, "signed_offline")?)?;
        }
        GovernanceCommand::Inspect {
            inputs,
            body,
            signed,
        } => {
            let inputs = inputs.load()?;
            let (body, status) = match (body, signed) {
                (Some(path), None) => (
                    inputs.body(read_json(&path, false)?)?.body().clone(),
                    "body_checked_offline",
                ),
                (None, Some(path)) => (inputs.signed(&path)?.body, "signature_checked_offline"),
                _ => return Err(anyhow!("select exactly one body or signed file")),
            };
            print_json(&describe(&body, &inputs.profile, status)?)?;
        }
        GovernanceCommand::Submit {
            inputs,
            endpoint,
            signed,
        } => {
            let inputs = inputs.load()?;
            let signed = inputs.signed(&signed)?;
            let client = client(&endpoint)?;
            let profile = client.query_profile().await?;
            let governance_view = client.query_governance_profile().await?;
            let account = client
                .query_account(&inputs.context.domain.account_id)
                .await?
                .context("account is absent from committed state")?;
            refresh(&inputs, &signed, &profile, &governance_view, &account)?;
            let result = client
                .submit_governance_sync(&signed, &inputs.profile, inputs.transport_bound()?)
                .await?;
            print_json(
                &json!({"status":if result.admitted() {"check_tx_accepted"} else {"check_tx_rejected"},
                "committed":false, "actual_charge_udrt":null, "check_tx":result}),
            )?;
            ensure!(
                result.admitted(),
                "CheckTx rejected the transaction; no committed result is claimed"
            );
        }
    }
    Ok(())
}

fn proposal(
    id: u64,
    network: AddressNetwork,
    change: ChangeArgs,
    owner: Option<String>,
) -> Result<Action> {
    ensure!(
        owner.is_none() || change.registry_add.is_some(),
        "--owner applies only to --registry-add"
    );
    let action = match change {
        ChangeArgs {
            max_active: Some(value),
            ..
        } => governance::parameter_change(id, &ParameterChange::MaxActive(value))?,
        ChangeArgs {
            min_self_bond_udgt: Some(value),
            ..
        } => governance::parameter_change(id, &ParameterChange::MinSelfBond(value))?,
        ChangeArgs {
            fees: Some(path), ..
        } => {
            let values: FeeValues = read_json(&path, false)?;
            governance::parameter_change(id, &ParameterChange::Fees(values))?
        }
        ChangeArgs {
            registry_add: Some(validator_id),
            ..
        } => {
            let owner = owner.context("--registry-add requires --owner")?;
            AccountAddress::decode(network, &owner)
                .map_err(|_| anyhow!("owner is not an account address on this network"))?;
            governance::registry_change(
                id,
                &RegistryChange::Add {
                    validator_id,
                    owner,
                },
            )?
        }
        ChangeArgs {
            registry_remove: Some(validator_id),
            ..
        } => governance::registry_change(id, &RegistryChange::Remove { validator_id })?,
        _ => return Err(anyhow!("select exactly one proposed change")),
    };
    Ok(action)
}

fn network(code: u8) -> Result<AddressNetwork> {
    match code {
        1 => Ok(AddressNetwork::Mainnet),
        2 => Ok(AddressNetwork::Testnet),
        3 => Ok(AddressNetwork::Development),
        _ => Err(anyhow!("unsupported network")),
    }
}

/// The chain moves on between capture and submission (it commits empty
/// blocks), so the refreshed views may be at a later height. They must show
/// the captured authority and both profiles, and the body must still be
/// valid for the next height; a proposal must still hold the next ID.
fn refresh(
    inputs: &Validated,
    signed: &SignedOrdinary,
    profile: &ProfileView,
    governance_view: &GovernanceProfileView,
    account: &AccountView,
) -> Result<()> {
    let (context, current) = governance::refresh_views(
        &inputs.context,
        &inputs.profile,
        profile,
        governance_view,
        account,
    )?;
    PreparedGovernance::from_body(signed.body.clone(), &current, &context)
        .context("the signed body is not valid for the next height")?;
    if let [Action::GovernanceProposal { proposal_id, .. }] = signed.body.actions.as_slice() {
        ensure!(
            *proposal_id == governance_view.next_proposal_id,
            "proposal ID {proposal_id} is taken; the next proposal ID is {}",
            governance_view.next_proposal_id
        );
    }
    Ok(())
}

fn describe(body: &OrdinaryTransaction, profile: &FeeProfileV3, status: &str) -> Result<Value> {
    let quote = FeeQuote::from_profile(&profile.base, body.gas_limit)?;
    Ok(json!({"status":status,
        "transaction_id":bytes_to_hex(&governance::transaction_id(body, &profile.limits())?),
        "maximum_fee_udrt":body.maximum_fee.to_string(), "required_cap_udrt":quote.required_cap.to_string(),
        "minimum_charge_udrt":quote.minimum_charge.to_string(), "actual_charge_udrt":null,
        "committed":false, "rule_failures_are_charged":true, "body":body}))
}

#[cfg(test)]
#[path = "governance_tests.rs"]
mod tests;
