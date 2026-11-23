# Notifications & Events

Use when answering: "Why didn't an email notification fire?", "Who was it sent to and why was it ignored/failed?", or "Which notification sent this email?"

All commands below are read-only and use `snow-cli-ro`.

## 1. Key tables

| Table | What it stores | Key fields to project |
|---|---|---|
| `sys_email` | Outbound & inbound email log | `sys_id,type,state,mailbox,subject,recipients,copied,instance,target_table,error_string,sys_created_on` |
| `sys_email_log` | Per-email processing & skip diagnostics | `email,notification,event,sys_created_on` |
| `sysevent` | Queued & processed system events | `sys_id,name,table,instance,state,processed,parm1,parm2,queue,sys_created_on` |
| `sysevent_register` | Event registry (valid event names) | `event_name,table,fired_by,description` |
| `sysevent_email_action` | Email Notification definitions | `sys_id,name,collection,active,generation_type,event_name,action_insert,action_update,condition,advanced_condition,weight,send_self,force_delivery,recipient_fields,recipient_users,recipient_groups,event_parm_1,event_parm_2` |
| `sysevent_in_action` | Inbound Email Actions | `name,table,type,order,active,condition_script,filter_condition,stop_processing` |
| `sys_properties` | Global email switches/redirects | `name,value` (`glide.email.smtp.active`, `glide.email.test.user`, `glide.email.read.active`) |

## 2. Step-by-step diagnosis (work backwards from the email)

### Step A — Check if an email record was created for the target record

`sys_email.instance` stores the `sys_id` of the record that triggered the notification (e.g. an Incident's `sys_id`).

```bash
# 1. Get the target record's sys_id (if you only have a number like INC0010001)
snow-cli-ro table get incident INC0010001 --fields sys_id,number,state,assigned_to

# 2. Check all email records linked to that record
snow-cli-ro table list sys_email \
  --query 'instance=<record_sys_id>' \
  --fields sys_id,type,state,subject,recipients,error_string,sys_created_on \
  --order-by -sys_created_on --limit 20

# 3. Check sys_email_log for which notification/event generated or processed the email
snow-cli-ro table list sys_email_log \
  --query 'email=<email_sys_id>' \
  --fields notification.name,event,sys_created_on --limit 20
```

Interpret `sys_email.type`:
- `sent`: handed off to mail server.
- `send-ready`: waiting on SMTP sender job (if stuck here, check `glide.email.smtp.active`).
- `send-ignored`: skipped (usually empty/inactive recipients, duplicate/weight suppression, or user opted out — check `error_string` and `sys_email_log`).
- `send-failed`: delivery error (check `error_string`).

### Step B — If no `sys_email` row exists, inspect matching Notification rules

```bash
# Find active notifications for the table
snow-cli-ro table list sysevent_email_action \
  --query 'collection=incident^active=true' \
  --fields sys_id,name,generation_type,event_name,action_insert,action_update,condition,weight,send_self,force_delivery,recipient_fields,event_parm_1,event_parm_2 \
  --order-by name --limit 50

# Read advanced condition script if present
snow-cli-ro table get sysevent_email_action <notif_sys_id> --fields advanced_condition --raw
```

`generation_type` tells you how the notification fires:
- `engine`: record insert/update (`action_insert`, `action_update`, `condition`).
- `event`: fired when `event_name` is processed in `sysevent`.
- `triggered`: fired directly from a Flow Designer / Workflow Studio action.

### Step C — If event-driven, verify the event actually fired and was processed

```bash
# Check recent events for that record
snow-cli-ro table list sysevent \
  --query 'instance=<record_sys_id>' \
  --fields sys_id,name,state,processed,parm1,parm2,sys_created_on \
  --order-by -sys_created_on --limit 20

# Find which code calls gs.eventQueue('<event_name>', ...)
snow-cli-ro codesearch search '<event_name>' --limit 20
```

### Step D — Check instance-wide email properties & recipient settings

```bash
# Outbound enabled? Sub-prod catch-all redirect configured?
snow-cli-ro table list sys_properties \
  --query 'nameINglide.email.smtp.active,glide.email.test.user,glide.email.read.active' \
  --fields name,value

# Verify recipient is active, has an email address, and has notifications enabled
snow-cli-ro table list sys_user \
  --query 'sys_id=<user_sys_id>' \
  --fields user_name,email,active,notification,locked_out
```

## 3. Gotchas

- **`send_self=false` (default):** ServiceNow does not email the user who made the update unless `send_self=true` on the notification. Very common during testing!
- **Notification `weight`:** When multiple notifications trigger on the same record for the same recipients, a higher `weight` (anything > 0) suppresses all lower-weight notifications (and weight `0` never suppresses others).
- **Outbound switch & sub-prod redirect:** If `glide.email.smtp.active=false`, outbound SMTP sending is disabled altogether (emails sit in `send-ready` regardless of `glide.email.test.user`). When SMTP is active and `glide.email.test.user` is set, outbound emails are redirected to that test address.
- **User opt-out (`sys_user.notification`):** Value `1` = Disabled, `2` = Enabled. Also check per-user subscriptions (`cmn_notif_message`) unless `force_delivery=true` on the notification.
- **Notifications do not inherit down to child tables:** A notification defined on `task` does not fire automatically for `incident` updates — always check notifications targeting the concrete child table.
- **Inbound email debugging:** Query `sys_email` with `typeSTARTSWITHreceived`, then inspect `sys_email_log` and active `sysevent_in_action` rules sorted by `order` (watch for `stop_processing=true`).
