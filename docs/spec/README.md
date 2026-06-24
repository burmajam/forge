# OMG specification Markdown

High-fidelity Markdown conversions of the OMG KerML / SysML v2 specification PDFs, for searchable,
grep-able reference (code blocks fenced, diagrams extracted as images, tables preserved).

One folder per spec; the Markdown and its extracted figures live together so the relative image
links resolve in place:

| Folder | Source PDF | Pages |
|--------|------------|-------|
| `1-Kernel_Modeling_Language/`        | KerML 1.0                         | 454 |
| `2a-OMG_Systems_Modeling_Language/`  | SysML v2                          | 691 |
| `2b-SysML_v1_to_v2_Transformation/`  | SysML v1 → v2 transformation      | 661 |
| `3-Systems_Modeling_API_and_Services/` | Systems Modeling API & Services | 107 |

Each folder contains `<name>.md`, the figures it links (`_page_<n>_Figure_<m>.jpeg`), and a
`<name>_meta.json` carrying the converter's page stats and table of contents.

## Provenance

Source PDFs: `vendor/SysML-v2-Release/doc/`. Converted with
[Marker](https://github.com/datalab-to/marker) (`marker-pdf`), chosen over Docling in a bake-off on
KerML §8.3.3 — Marker fenced each OCL/constraint body correctly, extracted every figure in order,
and kept the heading hierarchy, where Docling collapsed pages of attributes into run-on code fences
and dropped/mis-ordered figures.

Regenerate (Apple Silicon, MPS) from the gitignored tooling venv at `tools/pdfconv/`:

```bash
uv venv --python 3.13            # in tools/pdfconv/
uv pip install marker-pdf docling
TORCH_DEVICE=mps marker_single \
  vendor/SysML-v2-Release/doc/<spec>.pdf \
  --output_dir docs/spec --disable_ocr --disable_tqdm
```

`--disable_ocr` is intentional: these are digital PDFs with a text layer. Using it is faster and
more accurate than OCR, which split camelCase identifiers (`conjugatedType` → "conjugated Type")
and dropped italic emphasis.

## Caveats

- Read figures from the per-spec `.jpeg` files; for anything ambiguous, fall back to the source PDF
  via the Read tool's `pages` parameter.
- A few source-text-layer artifacts survive conversion (escaped `[0..\*]` multiplicities, the
  occasional `Type<./code>` fragment or typo). The authoritative grammar remains
  `vendor/SysML-v2-Release/bnf/KerML-textual-bnf.kebnf`.
