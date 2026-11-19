---
number: 58
title: The site and the skills speak to any host's operator, and name nothing of ours
status: accepted
date: 2026-10-26
decision-makers:
- Chuck Swanberg
---

# The site and the skills speak to any host's operator, and name nothing of ours

Status: accepted by the user in madswan-design's pane on 2026-10-26 ("People aren't going to know what the word "keeper" means or anything about SwanCloud machine configuration. This needs to be presented in a more generic way").

Deciders: the user, on a documentation chore whose words said "the keeper" and "swancloud's machine configuration", in madswan-design's pane on 2026-10-26, as quoted above; the record's other words are madswan-design's.
Sources: `agentplot/blueprints:design/sluice/requirements.md` clauses 35, 71 (which coin "the keeper"), and its givens ("sluice is free and open source"); rules core.1, core.13; the setting-up skill's step 6; `site/src/content/docs`.

Amended 2026-11-18: "the lead", the statement's word for the one host that moves a branch's queue (72, 73, 137), is such a word too. The user could not follow it on the pages: "i don't understand the "leads" part i see in the docs" (signal `2026-11-18-boomdrive-operator-33bf2bfc/01-sluice-lead-is-unclear`). boomdrive-design ruled that day that the pages, the skills, the commands' output and the CloudFormation template `sluice init codebuild` writes say the host that runs the queue, or the host that runs the branch's queue, where the statement says the lead; the hosts that may run its queue for the hosts a branch accepts for it; and the start role for what the template called the lead role, so `sluice step` says `elsewhere` where it said `led`, and the template's parameter and role are `StartPrincipal` and `StartRole`. The statement, the records, the rules and the code's identifiers keep "lead", as this record keeps "keeper" for them. The words are the change `sluice-names-the-host-that-runs-the-queue`'s, design D1 to D3; not taken: "the host that keeps the branch" for every use, which says less of what the host does.

## Context and problem statement

The statement writes for the family: it names the keeper, the host's shared caches it declares, the fleet that writes them. A chore to document the host's settings carried those words onto the site and into the setup skill, where a reader is anyone who installed sluice on a machine and knows nothing of our fleet, of swancloud, or of what the statement means by a keeper. Which words may the product's own pages use?

## Considered options

1. The site, the README, the plugins' skills and the commands' output speak to any host's operator: "whoever administers the host", "configuration management (Nix, Ansible, an MDM)", and never a word the statement coined that the reader cannot see in the product, nor a machine, fleet, partition or tool of ours.
2. The product's pages share the statement's vocabulary, with a glossary.
3. No rule; each writer judges.

## Decision outcome

Option 1. A glossary teaches a reader our words so that our pages can keep them, which is backwards: the product's pages exist for the reader. The statement and the records keep their vocabulary, since they are written for whoever builds sluice, and a rule is drawn from this record so that no chore carries the words across again. A host's settings file is described as written either by the person administering the host or by whatever manages the host's configuration, and the setup skill, finding the file managed, read-only or a link into a store, says to change it where it is managed, naming no manager of ours.

### Consequences

- Rule core.26 is drawn from this record.
- The chore `the-site-and-the-setup-skill-document-the-hosts-settings-and-who-writes-them` is reworded before it is built.
- The statement's "keeper" stays the statement's word.
