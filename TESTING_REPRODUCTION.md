# Testing Reproduction Guide

This document describes how to reproduce the account banning issue reported in [#112](https://github.com/NIyueeE/ds-free-api/issues/112) and collect meaningful data for analysis.

## Scenario Overview

**Original report**: 4 accounts, unique device_id per account, 2 requests/account, 6-10s delay between requests, same IP at registration.
**Result**: 1 banned in 10min, 2 banned in 12hrs, 1 survived.

## Prerequisites

1. **4 DeepSeek accounts** with unique device_ids (captured from real browser login)
2. **Server** running ds-free-api with test config
3. **API key** configured in Admin UI
4. **curl** or HTTP client for sending requests

## Configuration

Use `config.example.testing.toml` as base:

```bash
cp config.example.testing.toml config.toml
# Edit config.toml and fill in your 4 test accounts
```

Key testing settings:
- `hourly_request_quota = 10` (very conservative)
- `default_search_enabled = true`
- Only `model_types = ["default"]`

## Test Procedure

### 1. Start Server
```bash
RUST_LOG=ds_core::accounts=debug,adapter=debug cargo run
```

### 2. Verify Accounts Initialized
Check logs for:
```
INFO ds_core::accounts Account test1@example.com initialized successfully
INFO ds_core::accounts Account test2@example.com initialized successfully
...
```

### 3. Send Requests (Sequential Per Account)

For each account, send 2 requests with 6-10s delay:

```bash
# Request 1 for all 4 accounts
for i in {1..4}; do
  curl -X POST http://127.0.0.1:22217/v1/chat/completions \
    -H "Authorization: Bearer YOUR_API_KEY" \
    -H "Content-Type: application/json" \
    -d '{"model": "deepseek-default", "messages": [{"role": "user", "content": "Test message '$i' - request 1"}]}'
  sleep 2  # small gap between accounts
done

# Wait 6-10 seconds
sleep 8

# Request 2 for all 4 accounts
for i in {1..4}; do
  curl -X POST http://127.0.0.1:22217/v1/chat/completions \
    -H "Authorization: Bearer YOUR_API_KEY" \
    -H "Content-Type: application/json" \
    -d '{"model": "deepseek-default", "messages": [{"role": "user", "content": "Test message '$i' - request 2"}]}'
  sleep 2
done
```

### 4. Monitor for Bans

Watch server logs for:
- `biz_code=5` (account muted)
- `biz_code=11` (RISK_DEVICE_DETECTED)
- `quota_exhausted` warnings
- Session create/delete timing

Check account status via Admin API:
```bash
curl -H "Authorization: Bearer YOUR_ADMIN_JWT" \
  http://127.0.0.1:22217/admin/api/account-statuses-detailed
```

### 5. Extended Monitoring (12-24 hours)

Keep server running and periodically check:
- Account status changes
- Any delayed bans (original report: bans at 12hrs)

## Data to Collect

| Data Point | Source |
|------------|--------|
| Request timestamps per account | `ds_core::accounts` debug logs |
| Session create/delete time | `session_created` / `session_deleted` logs |
| PoW solve time | `pow_ms` in `sse_ready` log |
| First byte latency | `completion_ms` in `sse_ready` log |
| Upstream response codes | `biz_code` in error logs |
| Account quota usage | `account-statuses-detailed` endpoint |
| Request intervals | `recent_intervals` in account status |

## Expected Log Patterns

**Normal request flow:**
```
INFO ds_core::accounts req=req-X session_created: id=..., create_ms=..., account=...
INFO ds_core::accounts req=req-X sse_ready: resp_msg=..., pow_ms=..., completion_ms=..., session_create_ms=..., total_ms=...
INFO ds_core::accounts session_deleted: id=..., finished=true, cleanup_ms=...
```

**Rate limit / quota:**
```
WARN ds_core::accounts Account X quota exhausted (used=10, limit=10), skipping
WARN ds_core::accounts Account X reached the hourly request budget (10)
```

**Upstream errors:**
```
ERROR ds_core::accounts req=req-X SSE 流返回业务错误: biz_code=5, biz_msg=...
ERROR ds_core::accounts health_check 检测到业务错误: account=X, response=...
```

## Reporting Template

When reporting results, include:

```markdown
## Test Report - [Date]

### Setup
- Accounts: 4 (email/device_id pairs)
- Config: hourly_request_quota=10, sliding_window=true
- Delay between requests: ~8s
- Total requests per account: 2

### Results
| Account | Request 1 | Request 2 | Ban Time | Ban Reason |
|---------|-----------|-----------|----------|------------|
| acc1    | ✅ success | ✅ success | 10min    | biz_code=5 |
| acc2    | ✅ success | ✅ success | 12hr     | biz_code=5 |
| acc3    | ✅ success | ✅ success | 12hr     | biz_code=5 |
| acc4    | ✅ success | ✅ success | survived | - |

### Key Logs
[Link to gist or paste relevant log sections]

### Observations
- [Any patterns noticed]
- [Whether sliding window helped vs fixed window]
- [Session create/delete frequency observations]
```

## Next Steps After Reproduction

1. **Compare with fixed window** - temporarily revert to fixed window and re-run
2. **Test coordinated retry** - trigger 429 and observe retry behavior
3. **Test session reuse** - implement delayed session delete and re-run
4. **Document findings** - post to GitHub issue with data

## Safety Notes

- **Never share credentials** in logs or reports
- **Use separate device_ids** per account (critical!)
- **Monitor bans closely** - stop test if accounts getting banned rapidly
- **Backup accounts** - have spare accounts ready

## Questions?

Open an issue or discussion in the repo for help with setup.