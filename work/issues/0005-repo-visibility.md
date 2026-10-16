---
status: todo
type: question
created: 2026-10-16
---
# Private or public repo?

## Problem
github.com/helico-tech/pecunia is private. The web app's "Source on GitHub" links then give a
404, the release workflow's container image on ghcr.io is private (`k8s.yaml` would need a pull
secret), and CI on macOS and Windows runners uses billed minutes.

## Notes
OpenIssue is public. Decide, then fix the links or the docs.
