# Customer Service Management (CSM) Pack (`snow-cli`)

Opt-in pack for inspecting CSM Cases, Accounts, Contacts, Consumers, Entitlements, Account Relationships, and diagnosing external customer visibility issues.

All commands below are read-only and use `snow-cli-ro`.

## 1. Key tables

| Table | What it stores | Key fields to project |
|---|---|---|
| `sn_customerservice_case` | Customer Service Cases (extends `task`) | `sys_id,number,short_description,state,priority,account.name,contact.name,consumer.name,entitlement.name,product.name,assignment_group.name,active` |
| `customer_account` | B2B Customer Accounts (extends `core_company`) | `sys_id,name,customer,partner,account_parent.name,primary_contact.name,active` |
| `customer_contact` | B2B External Contacts (extends `sys_user`) | `sys_id,user_name,name,email,account.name,active,locked_out` |
| `csm_consumer` | B2C Consumers | `sys_id,name,user.user_name,email,active` |
| `service_entitlement` | Support Entitlements (hours/cases/SLA tier) | `sys_id,name,account.name,product.name,start_date,end_date,remaining_units,active` |
| `account_relationship` | Parent/partner account access links | `from_company.name,to_company.name,relationship_type.name,reverse_relationship` |
| `sn_query_rule` | CSM Query Rules (read filters when `sn_cs_queryrules.use_query_rules=true`) | `sys_id,name,table,active,sys_scope` |

## 2. Inspect a Case with its full B2B/B2C context

```bash
# 1. Inspect the Case
snow-cli-ro table get sn_customerservice_case CS0001001 \
  --fields sys_id,number,short_description,state,priority,account,account.name,contact,contact.name,consumer.name,entitlement.name,assignment_group.name \
  --display-value both

# 2. Check the Customer Account, hierarchy, and active Entitlements
snow-cli-ro table get customer_account <account_sys_id> \
  --fields name,customer,partner,account_parent.name,primary_contact.name,active

snow-cli-ro table list service_entitlement \
  --query 'account=<account_sys_id>^active=true' \
  --fields name,product.name,start_date,end_date,remaining_units --limit 20

# 3. Check Account Relationships (partner/child visibility)
snow-cli-ro table list account_relationship \
  --query 'from_company=<account_sys_id>^ORto_company=<account_sys_id>' \
  --fields from_company.name,to_company.name,relationship_type.name --limit 30
```

## 3. Diagnose "Why can/can't Contact X see Case Y?"

CSM external visibility depends on **three layers**: Contact roles, Account/Contact relationship, and CSM Query Rules (`sn_query_rule`) / Before Query BRs + ACLs on `sn_customerservice_case`.

```bash
# 1. Inspect the Contact and their active roles (look for sn_customerservice.customer vs customer_admin / partner)
snow-cli-ro table list customer_contact \
  --query 'user_name=julie.lewis' \
  --fields sys_id,user_name,name,account,account.name,active,locked_out

snow-cli-ro table list sys_user_has_role \
  --query 'user=<contact_sys_id>^state=active' \
  --fields role.name,inherited --all

# 2. Check CSM Query Rules (active when sn_cs_queryrules.use_query_rules=true) and Before Query BRs
snow-cli-ro table list sys_properties --query 'name=sn_cs_queryrules.use_query_rules' --fields name,value

snow-cli-ro table list sn_query_rule \
  --query 'table=sn_customerservice_case^active=true' \
  --fields sys_id,name,table,sys_scope.scope --limit 30

snow-cli-ro table list sys_script \
  --query 'collectionINsn_customerservice_case,task^when=before^action_query=true^active=true' \
  --fields sys_id,name,collection,order,condition,sys_scope.scope --order-by order --limit 30

# 3. Check CSM ACLs on sn_customerservice_case (read/write)
snow-cli-ro table list sys_security_acl \
  --query 'active=true^type=record^operation=read^nameINsn_customerservice_case,sn_customerservice_case.*' \
  --fields sys_id,name,decision_type,admin_overrides,condition,advanced --limit 50
```

## 4. Gotchas

- **Explicit Roles (`snc_external` vs `snc_internal`):** CSM requires the Explicit Roles plugin. External `customer_contact` users carry `snc_external` (not `snc_internal`); missing `snc_external` or adding `snc_internal` breaks portal & case visibility rules.
- **Query Rules (`sn_query_rule`) & Query BRs enforce Account/Contact scoping before ACLs:** Even if an ACL looks open, CSM Query Rules (when `sn_cs_queryrules.use_query_rules=true`) and Before Query Business Rules (`sn_customerservice_case`) filter cases to the contact's own cases (`sn_customerservice.customer`) vs all account cases (`sn_customerservice.customer_admin`) vs partner accounts.
- **Case is a scoped `task` extension (`sn_customerservice`):** Always check `sn_customerservice_case` AND `task` for Business Rules/ACLs, plus `sys_scope_privilege` / `sys_restricted_caller_access` if custom global/scoped scripts fail when touching Cases.
- **B2B (`account` + `contact`) vs B2C (`consumer`):** A Case can link to either `account`/`contact` or `consumer` (or household), so check all three fields before concluding customer context is empty.
