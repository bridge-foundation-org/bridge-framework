# Bridge Framework - Git Commit Strategy

**Purpose:** Establish a consistent, automated commit pattern for tracking feature implementation progress.

## Commit Message Format

Every feature/fix commit follows this structured template:

```
feat: [feature-name] - [short-description]

- Implementation detail 1
- Implementation detail 2
- Implementation detail 3
- Tests added: [N] new tests, [N] integration tests
- Files modified: [list of primary files]

All [total] tests passing.

Story Points: [X]
Phase: [N]
Closes #[issue] (if applicable)
```

### Real Example

```
feat: service registry - enable inter-service discovery

- Implemented ServiceRegistry struct with thread-safe storage
- Added service registration and deregistration methods
- Added service discovery (DNS fallback to localhost:port)
- Added health check mechanism with heartbeat
- Integrated with HTTP daemon on startup
- Tests added: 15 unit tests, 8 integration tests
- Files modified: daemon/src/registry.rs, daemon/src/http.rs, daemon/src/main.rs

All 423 tests passing.

Story Points: 3
Phase: 1a
```

## Commit Types

| Type | Usage | Example |
|------|-------|---------|
| `feat:` | New feature implementation | `feat: service registry - enable discovery` |
| `fix:` | Bug fix | `fix: race condition in rate limiter` |
| `test:` | Test suite additions | `test: add 45 integration tests for service calls` |
| `docs:` | Documentation updates | `docs: add service registry API documentation` |
| `refactor:` | Code reorganization | `refactor: simplify middleware chain logic` |
| `perf:` | Performance improvements | `perf: optimize trace buffer allocation` |
| `chore:` | Maintenance tasks | `chore: update dependencies` |

## Workflow: Feature → Commit

### Step 1: Plan (Review IMPLEMENTATION_PLAN.md)
```bash
# Read the feature requirements
# Check story points, complexity, tests needed
# Identify files to modify/create
```

### Step 2: Create Feature Branch
```bash
git checkout -b feat/service-registry
```

### Step 3: Write Tests First (TDD)
```bash
# Create daemon/tests/registry_tests.rs
# Write failing tests for all scenarios
cargo test --test registry_tests
# All fail ✅ (expected)
```

### Step 4: Implement Feature
```bash
# Create daemon/src/registry.rs
# Implement code to pass tests
cargo test --test registry_tests
# All pass ✅
```

### Step 5: Integration Tests
```bash
# Add integration test to daemon/tests/integration/
# Test with actual daemon running
cargo test --test '*'
```

### Step 6: Verify Everything
```bash
cargo check -p daemon              # Compile check
cargo clippy -p daemon             # Lint check
cargo test --workspace             # All tests
cargo fmt --check                  # Format check
```

### Step 7: Commit with Template
```bash
git add daemon/src/registry.rs daemon/tests/registry_tests.rs

git commit -m "feat: service registry - enable inter-service discovery

- Implemented ServiceRegistry struct
- Added registration/deregistration
- Added service discovery mechanism
- Tests added: 15 new tests
- Files modified: daemon/src/registry.rs, daemon/src/main.rs

All 423 tests passing.

Story Points: 3
Phase: 1a"
```

### Step 8: Push & Create PR
```bash
git push origin feat/service-registry

# Create PR with description of changes
# Link to IMPLEMENTATION_PLAN.md section
```

## Automated Commit Checklist

Before committing, verify:

- [ ] All new tests pass
- [ ] All existing tests still pass (`cargo test --workspace`)
- [ ] Code compiles without warnings (`cargo check -p daemon`)
- [ ] Code formatted (`cargo fmt`)
- [ ] No clippy warnings (`cargo clippy -p daemon`)
- [ ] Commit message follows template
- [ ] Story points and phase documented
- [ ] Files list is accurate

## Test Count Tracking

Each commit should update the test count. Format:

```
All [current_total] tests passing.
```

**Current Baseline:** 390 tests

**Phase 1a Target:** 423 tests (+33)  
**Phase 1b Target:** 443 tests (+20)  
**Phase 1c Target:** 468 tests (+25)  
**Phase 1d Target:** 498 tests (+30)  

## Story Points Per Commit

Most commits represent a sub-component:

| Component | Story Points | Tests Expected |
|-----------|--------------|-----------------|
| Service Registry | 3 | 15 unit + 8 integration |
| Service Discovery | 2 | 10 unit |
| HTTP Transport | 5 | 20 unit + 15 integration |
| Request Signing | 3 | 15 unit |

## Phase 1a Commits (Expected)

```
1. feat: service registry - enable discovery (3 pts, +23 tests)
2. feat: service discovery - DNS + local resolution (2 pts, +10 tests)
3. feat: HTTP transport layer - cross-service calls (5 pts, +35 tests)
4. feat: request signing - authenticate service calls (3 pts, +15 tests)
5. test: integration tests for service-to-service (0 pts, +20 tests)
Total: 13 story points, +103 tests → 493 total tests
```

## Pre-Commit Hook (Optional)

Create `.git/hooks/pre-commit` (executable):

```bash
#!/bin/bash
set -e

echo "🔍 Running pre-commit checks..."

# Format check
echo "  Checking formatting..."
cargo fmt --check

# Lint check
echo "  Running clippy..."
cargo clippy --all-targets --all-features

# Compile check
echo "  Checking compilation..."
cargo check --workspace

# Run tests
echo "  Running tests..."
cargo test --workspace --quiet

echo "✅ All checks passed!"
exit 0
```

## Commit History Example

```
* abc1234 - feat: request signing - authenticate service calls (3 pts, Phase 1a)
* def5678 - feat: HTTP transport layer - cross-service calls (5 pts, Phase 1a)
* ghi9012 - feat: service discovery - DNS resolution (2 pts, Phase 1a)
* jkl3456 - feat: service registry - enable discovery (3 pts, Phase 1a)
* mno7890 - feat: streaming infrastructure - SSE support (276 lines)
* pqr1234 - (main: 390 tests passing)
```

## Release Commits (v0.1.0, v1.0, etc.)

Major milestones get special commits:

```
release: v0.1.0 - Phase 1 complete

✅ PHASE 1: Foundation Hardening
- Service-to-service HTTP calls (13 pts)
- Request context management (8 pts)
- Service struct & DI (13 pts)
- Config schema generation (13 pts)
- Total: 47 story points

📊 Metrics:
- Tests: 498/498 passing (100%)
- Code: 0 warnings
- Coverage: 95%+
- Performance: <50ms latency

📝 Documentation:
- IMPLEMENTATION_PLAN.md updated
- API reference complete
- 3 example apps
- Getting started guide

🎯 Ready for Phase 2
```

## Troubleshooting

**Commit message too long?**
Use body text, not title. Keep title under 70 chars.

**Forgot to add files?**
```bash
git add .
git commit --amend --no-edit
```

**Need to edit last commit?**
```bash
git commit --amend -m "new message"
```

**Squash multiple commits?**
```bash
git rebase -i HEAD~3  # Last 3 commits
# Mark 'squash' on secondary commits
```

## References

- See `IMPLEMENTATION_PLAN.md` for feature details
- See task list for priorities
- See PHASE 1a breakdown for exact requirements

---

**Strategy Version:** 1.0  
**Created:** July 27, 2026  
**Status:** Active
