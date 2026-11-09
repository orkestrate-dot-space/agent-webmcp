name: Bug report
description: Something broken in agent-webmcp (CLI, MCP, polyfill, index)
title: "fix: "
labels: [bug]
body:
  - type: textarea
    id: what
    attributes:
      label: What broke
      description: One thing per report. Include the exact command and the full error output.
      placeholder: "agent-webmcp get example.com -> ..."
    validations:
      required: true
  - type: textarea
    id: repro
    attributes:
      label: How to reproduce
      description: Steps, starting from a clean state. Include browser + driver + version (e.g. Browse execute, agent-browser 0.41).
    validations:
      required: true
  - type: textarea
    id: expected
    attributes:
      label: Expected behavior
      description: What should have happened instead.
    validations:
      required: true
  - type: input
    id: version
    attributes:
      label: Version
      description: Output of `agent-webmcp --version` (or commit sha if unreleased).
    validations:
      required: true
