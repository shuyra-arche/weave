# Agent entry point

This repository follows the shared standards in shuyra-arche/.github.

At the start of each new session resolve the current main revision of
https://github.com/shuyra-arche/.github and read, at that revision:
CONTRIBUTING.md, docs/engineering/README.md, agent.md, code-organization.md,
testing.md (the last three under docs/engineering), and the task-relevant
standards, including branch.md and pull-request.md for publication.
Follow the startup procedure in agent.md.

Read this repository's README and applicable local guidance, and the relevant
design contracts in https://github.com/shuyra-arche/document-weave before
changing public semantics. Read actual contents and record revisions.
Report missing access rather than assuming a contract. Keep private tests
under tests/; the proc-macro crate root may include its external test module
conditionally. README doctests exercise public examples and compile failures.
