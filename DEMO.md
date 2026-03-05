# Markdown Feature Demo

A polished sample document for testing terminal Markdown rendering in `catmd`.

> [!NOTE]
> This demo intentionally focuses on features that currently render cleanly in the terminal.

## Table of Contents

1. [Headings and Text](#headings-and-text)
2. [Lists and Task Lists](#lists-and-task-lists)
3. [Links](#links)
4. [Blockquotes and Callouts](#blockquotes-and-callouts)
5. [Code and Syntax Highlighting](#code-and-syntax-highlighting)
6. [Tables](#tables)
7. [Final Notes](#final-notes)

---

## Headings and Text

### Heading Level 3

#### Heading Level 4

##### Heading Level 5

###### Heading Level 6

This paragraph contains **bold**, *italic*, ***bold italic***, ~~strikethrough~~, and `inline code`.

You can also combine styles like **bold with `code` inside** and links like [inline links](https://example.com).

---

## Lists and Task Lists

### Unordered List

- Project kickoff complete
- Documentation drafted
- Review items:
  - Confirm release date
  - Confirm owner for QA
  - Confirm rollout checklist

### Ordered List

1. Define scope
2. Build prototype
3. Run validation
4. Publish notes

### Task List

- [x] Create initial brief
- [x] Add code samples
- [ ] Final legal review
- [ ] Publish release blog

---

## Links

Inline link: [Project homepage](https://example.com "Example title")

Autolink: <https://github.com>

Email autolink: <team@example.com>

Reference-style link: [Release playbook][playbook]

---

## Blockquotes and Callouts

> A clear release note is better than a long release note.
>
> - Engineering Handbook

> [!TIP]
> Keep checklists short and observable.

> [!WARNING]
> Do not deploy irreversible data migrations without a rollback plan.

Nested quote:

> Quarter goals
> > Stability first
> > Improve developer feedback loops

---

## Code and Syntax Highlighting

Inline command: `catmd DEMO.md`

```bash
# Build and run
cargo build
cargo run -- DEMO.md
```

```rust
fn summarize(name: &str, tasks_done: usize) -> String {
    format!("{name} completed {tasks_done} tasks")
}

fn main() {
    println!("{}", summarize("Avery", 7));
}
```

```json
{
  "name": "catmd-demo",
  "version": "1.0.0",
  "features": ["tables", "code", "callouts"]
}
```

```diff
- status: pending
+ status: shipped
```

---

## Tables

| Milestone | Owner | Due Date   | Status      |
| :-------- | :---- | :--------- | :---------- |
| Spec      | Rina  | 2026-03-10 | Complete    |
| MVP       | Omar  | 2026-03-20 | In Progress |
| Launch    | Kai   | 2026-04-02 | Planned     |

| Left aligned | Center aligned | Right aligned |
| :----------- | :------------: | ------------: |
| alpha        |      beta      |           100 |
| gamma        |      delta     |           250 |

---

## Final Notes

This demo intentionally omits unsupported sections like math blocks so the rendered output stays clean and visual.

[playbook]: https://example.com/release-playbook "Release playbook"
