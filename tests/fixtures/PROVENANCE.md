---
created_on: 2026-10-14 08:51
last_modified: 2026-10-14 08:51
status: current
---

# Native fixture provenance

`links.pdf`, `planner.pdf`, and `sample.rm` are copied byte-for-byte from the sibling Rust `remarkable-render` fixtures. Their source is the MIT-licensed [go-remarkable-render](https://github.com/alexgorbatchev/go-remarkable-render) revision `61287a92812e50f9778bac2775a77ba2ca882aaa`; the root LICENSE preserves attribution.

- `links.pdf` uses upstream `buildLinkPDF` to encode internal page destinations, URI links, unsupported actions, rectangles, and bounded text.
- `planner.pdf` uses upstream `buildMinimalPDF` for planner headers and malformed/calendar edge cases.
- `sample.rm` uses upstream `buildSampleRMStrokes` for ballpoint and highlighter ink.

The sibling [provenance](../../../remarkable-render/tests/fixtures/PROVENANCE.md) describes the fixture generator and Go oracle. All cloud test responses, credentials, document UUIDs, and mutable server state are synthetic and local.
