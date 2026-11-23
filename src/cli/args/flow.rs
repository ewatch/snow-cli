use clap::{Args, Subcommand};

use crate::models::identifiers::RecordKey;

const FLOW_SHOW_AFTER_HELP: &str = "Examples:
  snow-cli flow show 'Change - Normal - Authorize'
  snow-cli flow show change__normal__authorize
  snow-cli flow show 2f33fe56731310108ef62d2b04f6a749 --output toon

Output:
  flow (sys_id, name, internal_name, type, status, active, scope, description), triggers (name, type,
  table, condition), structure (mode parallel|sequential, top_level step types, branches), steps in
  execution order, and the distinct action_types. Each step is {path,kind,type,note}: path \"1.2.3\"
  is the position in the tree, kind is logic|action|subflow, type the logic/action/subflow name, and
  note the condition label (If: \"Rejected\") or the step comment.

Notes:
  - Matches sys_hub_flow by sys_id, internal_name, or name (flows and subflows).
  - Reads sys_hub_trigger_instance, sys_hub_flow_logic, sys_hub_action_instance,
    sys_hub_sub_flow_instance, and sys_variable_value (labels): six requests; the published
    snapshot and the *_v2 tables are read only when the flow has no steps of its own.
  - Step inputs, data pills, and scripts are not shown; use table list on the tables above.";

/// Flow Designer flows.
#[derive(Args, Debug)]
pub struct FlowArgs {
    #[command(subcommand)]
    pub command: FlowCommands,
}

#[derive(Subcommand, Debug)]
pub enum FlowCommands {
    /// Show a flow as a compact tree: trigger, structure, and every step in order
    #[command(after_help = FLOW_SHOW_AFTER_HELP)]
    Show {
        /// Flow name, internal_name, or sys_id
        #[arg(value_name = "FLOW")]
        flow: RecordKey,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn parses_flow_show_by_name() {
        let cli = Cli::parse_from(["snow-cli", "flow", "show", "Change - Normal - Authorize"]);
        match cli.command {
            Commands::Flow(FlowArgs {
                command: FlowCommands::Show { flow },
            }) => assert_eq!(flow.to_string(), "Change - Normal - Authorize"),
            _ => panic!("Expected Flow command"),
        }
    }

    #[test]
    fn rejects_query_operators_in_flow_name() {
        assert!(Cli::try_parse_from(["snow-cli", "flow", "show", "a^ORname=b"]).is_err());
    }
}
