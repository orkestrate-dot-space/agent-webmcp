name: Feature request
description: Propose a change to agent-webmcp
title: "feat: "
labels: [enhancement]
body:
  - type: textarea
    id: problem
    attributes:
      label: Problem
      description: What can't you do today? Why does it belong in agent-webmcp (not in a driver)?
    validations:
      required: true
  - type: textarea
    id: proposal
    attributes:
      label: Proposed shape
      description: The verbs, flags, or index changes — and what you deliberately left out.
    validations:
      required: true
  - type: checkboxes
    id: scope
    attributes:
      label: Scope check
      options:
        - label: "This needs no browser ownership, no CDP, and no network beyond `sync`"
