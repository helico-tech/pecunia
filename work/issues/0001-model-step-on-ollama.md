---
status: todo
type: idea
created: 2026-10-16
---
# Model step on Ollama

## Problem
v1 classifies with own accounts, rules and memory only (ADR 0004). budget.rs also asks a System One
model on Ollama before Review, and names new categories with a chat model.

## Notes
Needs async work outside a command (the engine is synchronous) and a new ADR. Desktop and CLI
only, or the browser too (Ollama must allow the origin). Port from budget.rs
`classify/decision_model.rs`, `questions.rs`, `proposals.rs`.
