use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::IsTerminal;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli::args::{OutputFormat, SeedArgs, SeedCommands};
use crate::cli::output;
use crate::models::identifiers::TableName;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedSpec {
    version: u32,
    kind: String,
    #[serde(default)]
    seed: Option<u64>,
    tables: Vec<SeedTable>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedTable {
    name: String,
    #[serde(default)]
    count: Option<usize>,
    #[serde(default)]
    records: Vec<BTreeMap<String, Value>>,
    #[serde(default)]
    template: Option<BTreeMap<String, Value>>,
    #[serde(default)]
    references: BTreeMap<String, SeedReference>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedReference {
    table: String,
    strategy: String,
}

#[derive(Debug, Serialize)]
struct SeedPlan {
    kind: &'static str,
    version: u32,
    command: &'static str,
    seed: Option<u64>,
    strategy: &'static str,
    tables: Vec<PlannedTable>,
    total_records: usize,
}

#[derive(Debug, Serialize)]
struct PlannedTable {
    table: String,
    records: usize,
    depends_on: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SeedRun {
    kind: String,
    version: u32,
    run_id: String,
    instance: String,
    spec_seed: Option<u64>,
    records: Vec<SeedRunRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SeedRunRecord {
    table: String,
    sys_id: String,
}

#[derive(Debug, Serialize)]
struct SeedApplyOutput<'a> {
    kind: &'static str,
    version: u32,
    command: &'static str,
    run_id: &'a str,
    strategy: &'static str,
    created: usize,
    tables: &'a [PlannedTable],
}

#[derive(Debug, Deserialize)]
struct CreatedRecordResponse {
    result: serde_json::Value,
}

pub async fn handle(
    args: SeedArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<()> {
    match args.command {
        SeedCommands::Plan { file } => {
            ensure_json_output(format)?;
            let spec = read_seed_spec(&file)?;
            let plan = build_plan(spec)?;
            output::print_output(&plan, format)
        }
        SeedCommands::Apply { file, dry_run, yes } => {
            ensure_json_output(format)?;
            let spec = read_seed_spec(&file)?;
            let plan = build_plan(spec.clone())?;
            if dry_run {
                return output::print_output(&plan, format);
            }
            if !yes {
                confirm_seed_apply(plan.total_records)?;
            }
            apply_seed(profile, format, instance, timeout_secs, spec, plan).await
        }
        SeedCommands::Cleanup {
            run_id,
            dry_run,
            yes,
        } => {
            ensure_json_output(format)?;
            if dry_run {
                let run = read_run(&run_id)?;
                return output::print_output(&cleanup_preview(&run), format);
            }
            if !yes {
                confirm_seed_cleanup(&run_id)?;
            }
            cleanup_seed(profile, format, instance, timeout_secs, &run_id).await
        }
    }
}

fn ensure_json_output(format: &OutputFormat) -> anyhow::Result<()> {
    if matches!(
        format,
        OutputFormat::Json | OutputFormat::Text | OutputFormat::Auto
    ) {
        Ok(())
    } else {
        anyhow::bail!("Seed commands support only JSON, text, or auto output")
    }
}

fn read_seed_spec(path: &str) -> anyhow::Result<SeedSpec> {
    let bytes =
        fs::read(path).map_err(|error| anyhow::anyhow!("Cannot read seed spec: {error}"))?;
    let spec: SeedSpec = serde_json::from_slice(&bytes)
        .map_err(|error| anyhow::anyhow!("Invalid seed spec JSON: {error}"))?;
    if spec.version != 1 {
        anyhow::bail!("Unsupported seed spec version {}; expected 1", spec.version);
    }
    if spec.kind != "seed-spec" {
        anyhow::bail!("Invalid seed spec kind; expected `seed-spec`");
    }
    if spec.tables.is_empty() {
        anyhow::bail!("Seed spec must contain at least one table");
    }
    Ok(spec)
}

fn build_plan(spec: SeedSpec) -> anyhow::Result<SeedPlan> {
    let mut tables = BTreeMap::new();
    let mut dependencies: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut counts = BTreeMap::new();

    for definition in spec.tables {
        let table: TableName = definition
            .name
            .parse()
            .map_err(|error| anyhow::anyhow!("Invalid seed table name: {error}"))?;
        let name = table.to_string();
        if tables.contains_key(&name) {
            anyhow::bail!("Seed spec defines table `{name}` more than once");
        }
        if definition.count.is_some() && definition.template.is_none() {
            anyhow::bail!("Table `{name}` sets `count` but has no `template`");
        }
        if definition.count.is_none() && definition.template.is_some() {
            anyhow::bail!("Table `{name}` sets `template` but has no `count`");
        }
        let count = definition
            .count
            .unwrap_or(0)
            .checked_add(definition.records.len())
            .ok_or_else(|| anyhow::anyhow!("Record count overflow for table `{name}`"))?;
        if count == 0 {
            anyhow::bail!("Table `{name}` must define at least one record");
        }
        let mut deps = BTreeSet::new();
        for (field, reference) in definition.references {
            if field.is_empty() {
                anyhow::bail!("Table `{name}` has an empty reference field name");
            }
            let target: TableName = reference.table.parse().map_err(|error| {
                anyhow::anyhow!("Invalid referenced table for `{name}.{field}`: {error}")
            })?;
            if target.as_str() == name {
                anyhow::bail!("Table `{name}` cannot reference itself in this seed format");
            }
            if reference.strategy != "round_robin" {
                anyhow::bail!(
                    "Unsupported reference strategy `{}` for `{name}.{field}`",
                    reference.strategy
                );
            }
            deps.insert(target.to_string());
        }
        counts.insert(name.clone(), count);
        dependencies.insert(name.clone(), deps);
        tables.insert(name, ());
    }

    for (name, deps) in &dependencies {
        for dep in deps {
            if !tables.contains_key(dep) {
                anyhow::bail!(
                    "Table `{name}` references `{dep}`, which is not defined in the seed spec"
                );
            }
        }
    }

    let order = topological_order(&dependencies)?;
    let total_records = counts.values().try_fold(0usize, |total, count| {
        total
            .checked_add(*count)
            .ok_or_else(|| anyhow::anyhow!("Total record count overflow"))
    })?;
    let planned_tables = order
        .into_iter()
        .map(|table| PlannedTable {
            records: counts[&table],
            depends_on: dependencies[&table].iter().cloned().collect(),
            table,
        })
        .collect();

    Ok(SeedPlan {
        kind: "seed-plan",
        version: 1,
        command: "seed plan",
        seed: spec.seed,
        strategy: "table_api",
        tables: planned_tables,
        total_records,
    })
}

fn confirm_seed_apply(record_count: usize) -> anyhow::Result<()> {
    if !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "Seed apply requires confirmation. Re-run with --yes for non-interactive use."
        );
    }
    eprint!("Create {record_count} seed record(s)? [y/N] ");
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    if !answer.trim().eq_ignore_ascii_case("y") {
        anyhow::bail!("Seed apply aborted");
    }
    Ok(())
}

fn confirm_seed_cleanup(run_id: &str) -> anyhow::Result<()> {
    if !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "Seed cleanup requires confirmation. Re-run with --yes for non-interactive use."
        );
    }
    eprint!("Delete records from seed run {run_id}? [y/N] ");
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    if !answer.trim().eq_ignore_ascii_case("y") {
        anyhow::bail!("Seed cleanup aborted");
    }
    Ok(())
}

fn run_directory() -> PathBuf {
    crate::config::AppConfig::config_dir().join("seed-runs")
}

fn run_path(run_id: &str) -> anyhow::Result<PathBuf> {
    if run_id.is_empty()
        || !run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        anyhow::bail!("Invalid seed run ID");
    }
    Ok(run_directory().join(format!("{run_id}.json")))
}

fn read_run(run_id: &str) -> anyhow::Result<SeedRun> {
    let path = run_path(run_id)?;
    let bytes =
        fs::read(&path).map_err(|_| anyhow::anyhow!("Seed run `{run_id}` was not found"))?;
    let run: SeedRun = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("Seed run `{run_id}` has an invalid tracking file"))?;
    if run.run_id != run_id || run.kind != "seed-run" || run.version != 1 {
        anyhow::bail!("Seed run tracking file does not match `{run_id}`");
    }
    for record in &run.records {
        let _: TableName = record
            .table
            .parse()
            .map_err(|_| anyhow::anyhow!("Seed run `{run_id}` contains an invalid table name"))?;
        let _: crate::models::identifiers::SysId = record
            .sys_id
            .parse()
            .map_err(|_| anyhow::anyhow!("Seed run `{run_id}` contains an invalid record ID"))?;
    }
    Ok(run)
}

fn write_run(run: &SeedRun) -> anyhow::Result<()> {
    let directory = run_directory();
    fs::create_dir_all(&directory)?;
    let path = run_path(&run.run_id)?;
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, serde_json::to_vec(run)?)?;
    fs::rename(temp_path, path)?;
    Ok(())
}

fn cleanup_preview(run: &SeedRun) -> serde_json::Value {
    serde_json::json!({
        "kind": "seed-cleanup-plan",
        "version": 1,
        "command": "seed cleanup",
        "run_id": run.run_id,
        "record_count": run.records.len(),
        "records": run.records.iter().rev().collect::<Vec<_>>(),
    })
}

async fn apply_seed(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    spec: SeedSpec,
    plan: SeedPlan,
) -> anyhow::Result<()> {
    let mut client = crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
    let run_id = format!("seed-{}", uuid::Uuid::new_v4().simple());
    let mut run = SeedRun {
        kind: "seed-run".to_string(),
        version: 1,
        run_id,
        instance: client.base_url().to_string(),
        spec_seed: spec.seed,
        records: Vec::new(),
    };
    let definitions = spec
        .tables
        .into_iter()
        .map(|table| (table.name.clone(), table))
        .collect::<BTreeMap<_, _>>();
    let mut created_by_table: BTreeMap<String, Vec<String>> = BTreeMap::new();
    write_run(&run)?;

    for planned in &plan.tables {
        let definition = definitions.get(&planned.table).ok_or_else(|| {
            anyhow::anyhow!("Missing seed definition for table `{}`", planned.table)
        })?;
        let mut records = definition.records.clone();
        if let Some(template) = &definition.template {
            for index in 1..=definition.count.unwrap_or_default() {
                let mut generated = template.clone();
                for value in generated.values_mut() {
                    if let Some(text) = value.as_str() {
                        *value = Value::String(
                            text.replace("{{index}}", &index.to_string())
                                .replace("{{seed}}", &plan.seed.unwrap_or_default().to_string()),
                        );
                    }
                }
                records.push(generated);
            }
        }
        for (index, mut record) in records.into_iter().enumerate() {
            for (field, reference) in &definition.references {
                let targets = created_by_table.get(&reference.table).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Referenced table `{}` has not been created",
                        reference.table
                    )
                })?;
                if targets.is_empty() {
                    anyhow::bail!(
                        "Referenced table `{}` contains no created records",
                        reference.table
                    );
                }
                record.insert(
                    field.clone(),
                    Value::String(targets[index % targets.len()].clone()),
                );
            }
            let body = serde_json::to_string(&record)?;
            let path = format!("/api/now/table/{}", planned.table);
            let tracking_path = run_path(&run.run_id)?;
            let response: CreatedRecordResponse =
                client.post_json(&path, &body).await.map_err(|error| {
                    anyhow::anyhow!(
                        "Seed run {} partially applied; cleanup tracking is at {}: {error}",
                        run.run_id,
                        tracking_path.display()
                    )
                })?;
            let sys_id = response.result.get("sys_id").and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("ServiceNow created a record in `{}` without returning sys_id; run {} may need manual cleanup", planned.table, run.run_id))?.to_string();
            run.records.push(SeedRunRecord {
                table: planned.table.clone(),
                sys_id: sys_id.clone(),
            });
            created_by_table
                .entry(planned.table.clone())
                .or_default()
                .push(sys_id);
            write_run(&run)?;
        }
    }

    let result = SeedApplyOutput {
        kind: "seed-apply-result",
        version: 1,
        command: "seed apply",
        run_id: &run.run_id,
        strategy: "table_api",
        created: run.records.len(),
        tables: &plan.tables,
    };
    output::print_output(&result, format)
}

async fn cleanup_seed(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    run_id: &str,
) -> anyhow::Result<()> {
    let mut run = read_run(run_id)?;
    let mut client = crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
    if client.base_url() != run.instance {
        anyhow::bail!("Seed run `{run_id}` belongs to a different ServiceNow instance");
    }
    let mut deleted = 0usize;
    while let Some(record) = run.records.last().cloned() {
        let path = format!("/api/now/table/{}/{}", record.table, record.sys_id);
        client.delete(&path).await?;
        run.records.pop();
        deleted += 1;
        write_run(&run)?;
    }
    fs::remove_file(run_path(run_id)?)?;
    output::print_output(
        &serde_json::json!({
            "kind": "seed-cleanup-result",
            "version": 1,
            "command": "seed cleanup",
            "run_id": run_id,
            "deleted": deleted,
        }),
        format,
    )
}

fn topological_order(
    dependencies: &BTreeMap<String, BTreeSet<String>>,
) -> anyhow::Result<Vec<String>> {
    let mut remaining = dependencies.clone();
    let mut order = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .find(|(_, deps)| deps.iter().all(|dep| order.contains(dep)))
            .map(|(name, _)| name.clone());
        match ready {
            Some(name) => {
                remaining.remove(&name);
                order.push(name);
            }
            None => {
                let cyclic = remaining.keys().cloned().collect::<Vec<_>>().join(", ");
                anyhow::bail!(
                    "Seed table references contain a dependency cycle involving: {cyclic}"
                );
            }
        }
    }
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(json: &str) -> SeedSpec {
        serde_json::from_str(json).expect("valid test seed spec")
    }

    #[test]
    fn plan_orders_dependencies_and_counts_explicit_and_generated_records() {
        let plan = build_plan(spec(
            r#"{"version":1,"kind":"seed-spec","seed":42,"tables":[
                {"name":"incident","count":2,"template":{"short_description":"Generated {{index}}"},
                 "references":{"location":{"table":"cmn_location","strategy":"round_robin"}}},
                {"name":"cmn_location","records":[{"name":"Berlin"},{"name":"Boston"}]}
            ]}"#,
        ))
        .expect("plan should be valid");
        assert_eq!(plan.total_records, 4);
        assert_eq!(plan.seed, Some(42));
        assert_eq!(plan.tables[0].table, "cmn_location");
        assert_eq!(plan.tables[1].table, "incident");
        assert_eq!(plan.tables[1].records, 2);
        assert_eq!(plan.tables[1].depends_on, vec!["cmn_location"]);
    }

    #[test]
    fn plan_rejects_missing_reference_tables() {
        let error = build_plan(spec(
            r#"{"version":1,"kind":"seed-spec","tables":[
                {"name":"incident","count":1,"template":{},"references":{"location":{"table":"cmn_location","strategy":"round_robin"}}}
            ]}"#,
        ))
        .expect_err("missing dependency should fail");
        assert!(error.to_string().contains("not defined"));
    }

    #[test]
    fn plan_rejects_dependency_cycles() {
        let error = build_plan(spec(
            r#"{"version":1,"kind":"seed-spec","tables":[
                {"name":"incident","count":1,"template":{},"references":{"location":{"table":"cmn_location","strategy":"round_robin"}}},
                {"name":"cmn_location","count":1,"template":{},"references":{"incident":{"table":"incident","strategy":"round_robin"}}}
            ]}"#,
        ))
        .expect_err("cycle should fail");
        assert!(error.to_string().contains("dependency cycle"));
    }

    #[test]
    fn plan_rejects_unsupported_reference_strategy() {
        let error = build_plan(spec(
            r#"{"version":1,"kind":"seed-spec","tables":[
                {"name":"cmn_location","records":[{"name":"Berlin"}]},
                {"name":"incident","count":1,"template":{},"references":{"location":{"table":"cmn_location","strategy":"random"}}}
            ]}"#,
        ))
        .expect_err("unsupported strategy should fail");
        assert!(error.to_string().contains("Unsupported reference strategy"));
    }
}
