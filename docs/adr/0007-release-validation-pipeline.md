# ADR-0007: Release Validation Pipeline

## Status

Accepted

## Context

Release readiness requires evidence that the release candidate meets its intended
behavior, that user documentation reflects that behavior, and that the final
change is reviewed. Running these activities in an arbitrary order wastes work
and can leave documentation based on a superseded candidate.

Some snow-cli operations require a ServiceNow instance or a browser helper. The
release process must distinguish tests that can run locally from optional live
instance checks, without treating unavailable external infrastructure as passing
evidence.

## Decision

Every release candidate follows these gates in order:

1. The reviewer reviews the candidate against its fixed point, specification,
   and repository standards.
2. The E2E tester runs the approved command matrix on the reviewed candidate
   and produces sanitized artifacts for each scenario.
3. The documentation maintainer updates user-facing documentation using only
   successful E2E artifacts and the implemented behavior.
4. The release manager verifies the three gates, version metadata, release
   notes, packaging checks, and distribution prerequisites.

A code or behavior change after the reviewer gate invalidates the subsequent
gates. Review, E2E testing, and documentation must run again for the changed
candidate.

Local SN-Utils bridge protocol tests are required release evidence. Live
ServiceNow or browser-helper smoke tests are separately reported as passed,
failed, or unavailable; unavailable is not equivalent to passed.

Creating a GitHub release, pushing a tag, publishing assets, and updating a
Homebrew tap require explicit human approval after the release manager declares
the candidate ready.

## Alternatives Considered

1. Run E2E testing before review.
   - Rejected because known implementation or specification defects can make
     E2E runs and generated documentation artifacts obsolete.
2. Let documentation be written from source code or manually invented examples.
   - Rejected because user-facing command examples need execution evidence.
3. Require a live ServiceNow instance for every bridge check.
   - Rejected because WebSocket protocol behavior can be validated locally and
     a live instance is not always available.

## Consequences

### Positive

- Documentation examples have reproducible execution evidence.
- Expensive E2E runs occur only after a review gate.
- Release publication has an explicit approval boundary.
- Local bridge protocol coverage remains available without ServiceNow access.

### Trade-offs

- A late behavior change repeats three validation activities.
- Release preparation requires preserving sanitized E2E artifacts.
- Live instance coverage remains conditional on staging infrastructure.
