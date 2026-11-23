# Plugin & Store App Installation Pack (`snow-cli`)

Opt-in pack for inspecting installed or available ServiceNow plugins and Store applications, activating plugins or installing Store apps via the CI/CD REST API (`sn_cicd`) or Global background scripts (`GlidePluginManagerWorker`), tracking async progress, and troubleshooting plugin activations.

Read-only inspection, progress polling, and log queries work with `snow-cli-ro`. Activation, installation, and rollback commands (`api post`, `script run`) are **privileged write/admin operations** that require the full `snow-cli` binary (blocked by `snow-cli-ro` and `--read-only`). **Never activate, install, upgrade, or roll back a plugin or Store app without explicit user confirmation of the target instance, plugin/app ID, target version, and whether demo data should be loaded.**

## 1. Pre-flight checks (read-only — always run first)

```bash
# 1. Confirm target instance, user, and build before any install
snow-cli-ro auth status --verify

# 2. Check installed/known plugins and Store apps matching a keyword
snow-cli-ro scope list <search> --kind plugin --kind store-app

# 3. Check a platform plugin in v_plugin — the primary lookup; readable by admin over the Table API
#    (active is a string: active | inactive | upgradable)
snow-cli-ro table list v_plugin \
  --query 'id=<plugin_id>' \
  --fields id,name,active,version,available_version,scope --limit 5

# 4. Optional: persisted plugin rows. sys_plugins, sys_store_app and sys_upgrade_history_log often return
#    ACL_DENIED (403) over the Table API even for admin. If so, rely on v_plugin / `scope list`, or check with
#    `script run --code 'gs.info(GlidePluginManager.isActive("<plugin_id>"));'`
snow-cli-ro table list sys_plugins \
  --query 'source=<plugin_id>' \
  --fields source,name,active,version,install_date --limit 5

# 5. Check installed vs available Store / App Repo applications and versions (sys_store_app may be ACL_DENIED; see step 4)
snow-cli-ro table list sys_store_app \
  --query 'scope=<scope>^ORnameLIKE<search>' \
  --fields sys_id,name,scope,version,latest_version,active,install_date --limit 20
snow-cli-ro table list sys_remote_app \
  --query 'scope=<scope>^ORnameLIKE<search>' \
  --fields sys_id,name,scope,latest_version --limit 20
snow-cli-ro table list sys_app_version \
  --query 'remote_application=<remote_app_sys_id>' \
  --fields sys_id,version,publish_date,dependencies \
  --order-by -publish_date --limit 10

# 6. Check whether the CI/CD REST API (com.glide.continuousdelivery) properties are enabled
snow-cli-ro table list sys_properties \
  --query 'nameINsn_cicd.enabled,sn_cicd.plugin.activate.enabled,sn_cicd.plugin.rollback.enabled' \
  --fields name,value --limit 10
```

## 2. Activate plugins or install Store apps via CI/CD REST API (preferred — guarded write op)

Requires `admin` or `sn_cicd.sys_ci_automation` and the `com.glide.continuousdelivery` plugin (`sn_cicd.enabled=true`). Requests run asynchronously on the Application Operations Queue and return `result.links.progress.id`.

> `snow-cli api post` requires a request body (`--data`); pass `--data '{}'` for CI/CD endpoints that take no JSON payload.

```bash
# Activate a single platform plugin (no demo data)
snow-cli api post /api/sn_cicd/plugin/<plugin_id>/activate --data '{}'

# Install or upgrade a single Store app (use base_app_version for unaltered Store apps; version for custom apps/customizations)
snow-cli api post '/api/sn_cicd/app_repo/install?sys_id=<store_app_sys_id>&base_app_version=<version>' --data '{}'
snow-cli api post '/api/sn_cicd/app_repo/install?scope=<scope>&base_app_version=<version>' --data '{}'

# Batch install plugins and/or Store apps (also required to set load_demo_data: true for a plugin via CI/CD)
snow-cli api post /api/sn_cicd/app/batch/install \
  --data '{"name":"Plugin install","packages":[{"id":"<plugin_id>","type":"plugin","load_demo_data":false}]}'

# Roll back a plugin or batch (only when explicitly requested and a rollback context exists)
snow-cli api post /api/sn_cicd/plugin/<plugin_id>/rollback --data '{}'
snow-cli api post /api/sn_cicd/app/batch/rollback/<rollback_id> --data '{}'
```

## 3. Track async progress & verify completion (read-only)

Do not assume installation succeeded when `POST` returns (`status: "0"` / `Pending`), and do not rely solely on `v_plugin.active` or `sys_plugins.install_date` while background steps are still running.

```bash
# 1. Poll CI/CD progress until status is terminal ("0" Pending, "1" Running, "2" Successful, "3" Failed, "4" Canceled)
snow-cli-ro api get /api/sn_cicd/progress/<progress_id>
snow-cli-ro api get /api/sn_cicd/app/batch/results/<result_id>

# 2. Inspect Execution Trackers (CI/CD, batch installs, GlideMultiPluginManagerWorker)
snow-cli-ro table list sys_execution_tracker \
  --query 'nameLIKE<plugin_id>' \
  --fields sys_id,name,state,percent_complete,message,detail_message,start_time,completion_time \
  --order-by -sys_created_on --display-value both --limit 10

# 3. Inspect Progress Workers (GlidePluginManagerWorker / plugin loads)
snow-cli-ro table list sys_progress_worker \
  --query 'nameLIKE<plugin_id>' \
  --fields sys_id,name,state,state_code,message,error_message,sys_updated_on \
  --order-by -sys_created_on --limit 10

# 4. Inspect detailed plugin activation logs and any skipped upgrade items
#    (sys_upgrade_history_log may return ACL_DENIED over the Table API; open it in the UI instead)
snow-cli-ro table list sys_plugin_log \
  --query 'plugin=<plugin_id>' \
  --fields sys_created_on,log,sys_created_by \
  --order-by -sys_created_on --limit 30
snow-cli-ro table list sys_upgrade_history_log \
  --query 'disposition=4^resolution_status=not_reviewed' \
  --fields file_name,target_name,type,disposition,resolution_status,sys_created_on \
  --order-by -sys_created_on --limit 30
```

## 4. Fallback via Global background script (when `sn_cicd` is unavailable — guarded write op)

Use only if the instance lacks `sn_cicd` (404/403) and the user explicitly approves a Global background script. `GlidePluginManagerWorker` activates dependent plugins automatically and tracks execution in `sys_progress_worker` (add `w.setIncludeDemoData(true);` only if demo data was explicitly requested).

```bash
# Check activation status when v_plugin returns HTTP 500 (common on PDIs) and sys_plugins returns 403
snow-cli script run --scope global --code 'gs.info(GlidePluginManager.isActive("<plugin_id>"));'

# Activate a single plugin asynchronously (returns sys_progress_worker sys_id)
snow-cli script run --scope global --code 'var w = new GlidePluginManagerWorker(); w.setPluginId("<plugin_id>"); w.setProgressName("Activate <plugin_id>"); w.setBackground(true); w.start(); gs.info("progress_id=" + w.getProgressID());'

# Activate multiple plugins in one job (avoids concurrent worker contention; track in sys_execution_tracker)
snow-cli script run --scope global --code 'var w = new GlideMultiPluginManagerWorker(); w.setPluginIds(["<plugin_id_1>","<plugin_id_2>"]); w.setProgressName("Batch activate plugins"); w.setBackground(true); w.start();'
```

## 5. Gotchas

- **Many plugin activations are irreversible:** Platform plugins generally cannot be deactivated once installed, and `sn_cicd` rollback only works when a rollback context / prior state exists. Always verify the target instance with `auth status --verify` and get explicit confirmation first.
- **Default to no demo data:** Keep `load_demo_data: false` (and omit `setIncludeDemoData(true)`) unless the user explicitly asks for demo data.
- **`snow-cli api post` requires `--data`:** Pass `--data '{}'` even on bodyless CI/CD endpoints (`/activate`, `/rollback`, `/app_repo/install`), otherwise `snow-cli` exits locally with a validation error.
- **Entitlements & PDIs:** Paid/entitled Store apps and restricted plugins require instance entitlement and Store sync; on Personal Developer Instances (PDIs), many plugins must be activated from the ServiceNow Developer Portal rather than in-instance APIs, and `v_plugin` frequently returns HTTP 500 over the Table API.
- **Upgrade windows & queueing:** CI/CD installs use the Application Operations Queue and pause during scheduled upgrade windows (default 2 hours before upgrade); a pending queued job (`state=0`) can be canceled in `sys_execution_tracker` before it starts running.
- **Do not run parallel `GlidePluginManagerWorker` jobs:** Use `/api/sn_cicd/app/batch/install` or `GlideMultiPluginManagerWorker` when installing multiple plugins so dependencies resolve cleanly without lock contention.
