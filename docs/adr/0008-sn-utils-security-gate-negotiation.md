# ADR-0008: SN-Utils Security-Gate Negotiation

## Status

Accepted

## Context

SN-Utils helpers now expose per-instance security gates and a browser review
queue. Older helper builds either return capabilities without gate support or do
not implement capability negotiation. Sending a mutation without understanding
a current helper's gate state creates confusing timeouts and can bypass useful
fail-fast protection, while rejecting every older helper would break existing
installations.

The helper tab remains a separate security boundary. Its gate settings and
single-use approval workflow belong to the browser extension, not snow-cli.
Instance origins also differ syntactically: JavaScript's `URL.origin` omits the
default HTTPS port while snow-cli's session keys include `:443`.

## Decision

The broker-owned WebSocket manager sends a minimal `hostHello`, records helper
build/license metadata and revisioned gate snapshots, and clears all helper state
when the WebSocket generation changes. Incoming origins are canonicalized to the
same explicit-port form as broker session origins. Revisions are compared only
within one connection generation.

Immediately before each gated browser action, the broker actively sends
`agentGetCapabilities` and classifies the action at the broker dispatch seam:

- background-script execution uses `backgroundScripts`;
- record or attachment creation uses `createArtifacts`;
- REST deletion uses `deleteRecords`;
- other REST writes use `restRequest`;
- screenshots and CDP operations use `browserDebugger`;
- reads and helper/browser navigation actions without an extension gate are not
  gated by snow-cli.

A current helper that advertises `instanceSecurityGates` must return refreshed
state containing the selected instance. Missing instance state is unauthorized;
unrefreshable or unresolvable state fails closed for gated actions. `off` fails
before the action is written to the socket. `approve` and `auto` are dispatched:
`approve` deliberately lets the browser display and own its Review Queue, and a
response timeout says browser approval was awaited.

A helper that answers capability negotiation without gate support is labeled
`legacy_unrestricted`. A helper with no gate metadata that does not implement the
capability request is treated the same after the bounded negotiation timeout.
This is a compatibility boundary, not a claim that the helper provides current
security controls. Passive status may remain `unknown` until negotiation occurs.

Helper-reported `E_PAUSED` is represented separately from an action failure.
Preflight/status data is redacted and never contains `g_ck`, scripts, request
payloads, cookies, review nonces, or approval payload hashes.

Snow-cli never modifies extension gates, approves a review, replays an approval,
or treats preflight as the final enforcement authority. The browser extension
always rechecks and enforces its own state.

## Alternatives Considered

1. **Always dispatch and rely only on the extension.** Rejected because blocked
   actions produce avoidable failures/timeouts and agents cannot explain the
   instance's effective gate state.
2. **Reject helpers without gate negotiation.** Rejected because it would break
   established older-helper workflows without improving those helpers.
3. **Cache gate state for the broker lifetime.** Rejected because users can
   change gates at runtime and revisions are meaningful only for one WebSocket
   generation.
4. **Have snow-cli drive the review UI or change gate settings.** Rejected
   because it would cross the human-approval security boundary.

## Consequences

- Blocked current-helper mutations fail before sensitive payloads reach the
  browser socket.
- Approval-required actions remain interactive and browser-authoritative.
- Current helpers fail closed when mutation gate state is unavailable or the
  selected instance is unauthorized.
- Older helpers remain usable but are explicitly identified as unrestricted
  legacy compatibility.
- Gated actions incur a small capability-refresh round trip.
- New mutating helper protocol actions must be added to the centralized action
  classification and covered by broker-seam tests.
