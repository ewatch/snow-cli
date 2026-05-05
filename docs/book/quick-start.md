# Quick start

This page shows a minimal first-time workflow.

## 1. Create a profile

```bash
snow-cli profile add dev \
  --instance https://your-instance.service-now.com \
  --auth-method basic \
  --username your-user
```

You can also use the legacy alias:

```bash
snow-cli config add dev --instance https://your-instance.service-now.com --auth-method basic --username your-user
```

## 2. Store credentials

```bash
snow-cli auth login
```

For non-interactive use, pass a secret explicitly or through your automation environment:

```bash
snow-cli auth login --password '<password>'
```

## 3. Check authentication

```bash
snow-cli auth status
```

## 4. List records

```bash
snow-cli table list incident --query 'active=true' --limit 20
```

## 5. Get one record

```bash
snow-cli table get incident <sys_id>
```

## 6. Create a record

```bash
snow-cli table create incident --data '{"short_description":"Created from snow-cli"}'
```

You can also pipe JSON through stdin:

```bash
echo '{"short_description":"Created from stdin"}' | snow-cli table create incident
```

## 7. Choose output format

JSON is the default:

```bash
snow-cli table list incident --limit 5
```

Other supported formats include CSV, JSON Lines, TOON, and text where applicable:

```bash
snow-cli --output csv table list incident --fields number,short_description --limit 5
snow-cli --output jsonl table list incident --limit 5
snow-cli --output toon table list incident --limit 5
```
