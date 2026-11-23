# Knowledge Management & User Criteria Pack (`snow-cli`)

Opt-in pack for diagnosing Knowledge Bases, Knowledge Articles, and User Criteria visibility ("Why can/can't user X read or contribute to KB article Y?").

All commands below are read-only and use `snow-cli-ro`.

## 1. Key tables

| Table | What it stores | Key fields to project |
|---|---|---|
| `kb_knowledge` | Knowledge articles & versions | `sys_id,number,short_description,kb_knowledge_base.title,kb_category.label,workflow_state,valid_to,active,version.version,published,can_read_user_criteria,cannot_read_user_criteria` |
| `kb_knowledge_base` | Knowledge Base containers | `sys_id,title,active,owner.name,kb_managers,kb_version,disable_commenting` |
| `kb_uc_can_read_mtom` | KB-level "Can Read" User Criteria | `kb_knowledge_base.title,user_criteria,user_criteria.name` |
| `kb_uc_cannot_read_mtom` | KB-level "Cannot Read" User Criteria | `kb_knowledge_base.title,user_criteria,user_criteria.name` |
| `kb_uc_can_contribute_mtom` | KB-level "Can Contribute" User Criteria | `kb_knowledge_base.title,user_criteria,user_criteria.name` |
| `kb_uc_cannot_contribute_mtom` | KB-level "Cannot Contribute" User Criteria | `kb_knowledge_base.title,user_criteria,user_criteria.name` |
| `user_criteria` | Reusable audience rules (users, groups, roles, dept, script) | `sys_id,name,active,match_all,user,group,role,department,company,location,advanced,script` |

## 2. Diagnose "Why can't user X see article KB0010001?"

Check in this exact order: **article state & expiry** → **article-level User Criteria** → **KB-level Cannot Read / Can Read User Criteria** → **ACLs / Domain**.

```bash
# 1. Inspect the article's state, valid_to date, active flag, KB, and article-level User Criteria
snow-cli-ro table list kb_knowledge \
  --query 'number=KB0010001' \
  --fields sys_id,number,short_description,kb_knowledge_base,kb_knowledge_base.title,workflow_state,valid_to,active,can_read_user_criteria,cannot_read_user_criteria \
  --order-by -sys_updated_on --limit 10

# 2. Check KB-level Cannot Read and Can Read User Criteria (Cannot Read wins over Can Read!)
snow-cli-ro table list kb_uc_cannot_read_mtom \
  --query 'kb_knowledge_base=<kb_sys_id>' \
  --fields user_criteria,user_criteria.name --limit 50

snow-cli-ro table list kb_uc_can_read_mtom \
  --query 'kb_knowledge_base=<kb_sys_id>' \
  --fields user_criteria,user_criteria.name --limit 50

# 3. Inspect a User Criteria record's conditions (and script if advanced=true)
snow-cli-ro table get user_criteria <uc_sys_id> \
  --fields name,active,match_all,user,group,role,department,company,location,advanced \
  --display-value both

snow-cli-ro table get user_criteria <uc_sys_id> --fields script --raw

# 4. Compare against the user's profile attributes & memberships
snow-cli-ro table list sys_user \
  --query 'user_name=abel.tuter' \
  --fields sys_id,user_name,active,department.name,company.name,location.name,sys_domain
```

## 3. Audit KB health & article distribution

```bash
# Count articles per Knowledge Base and workflow_state
snow-cli-ro table stats kb_knowledge --group-by kb_knowledge_base,workflow_state

# Find published articles whose valid_to date has already expired
snow-cli-ro table list kb_knowledge \
  --query 'workflow_state=published^valid_to<javascript:gs.beginningOfToday()' \
  --fields number,short_description,kb_knowledge_base.title,valid_to \
  --order-by valid_to --limit 30
```

## 4. Gotchas

- **Multiple versions share the same `number`:** With Knowledge Versioning enabled, `KB0010001` can have several `kb_knowledge` rows (`draft`, `published`, `outdated`, `retired`). Use `table list kb_knowledge --query 'number=KB0010001'` instead of `table get` so you see every version.
- **`Cannot Read` overrides `Can Read`:** If a user matches *any* `Cannot Read` User Criteria on the KB or article, access is denied even if they also match `Can Read`.
- **Empty `Can Read` depends on `glide.knowman.block_access_with_no_user_criteria`:** By default (`false`), if a KB has no `kb_uc_can_read_mtom` rows and the article has empty `can_read_user_criteria`, any authenticated user who passes `kb_knowledge` ACLs (and isn't in `Cannot Read`) can read it. When that property is `true`, an empty `Can Read` (or `Can Contribute`) list denies access instead.
- **`match_all=false` (OR) vs `match_all=true` (AND):** On `user_criteria`, `match_all=false` means matching *any* populated field (user OR group OR role OR department) satisfies the criterion; `match_all=true` requires *all* populated fields to match.
- **Expired `valid_to` hides published articles:** Even when `workflow_state=published`, an article whose `valid_to` is in the past disappears from end-user search and portal views.
