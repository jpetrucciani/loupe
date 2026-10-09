# Selection candidate provenance

The shared core candidate module assigns identities over a finite UTF-8 document before selection or tournament reduction. Candidate text borrows the immutable original source. It does not normalize, trim parser spans or invent text. A physical line index plus per-candidate ranges/locations consume additional memory; line reading also uses temporary buffers. Find/pick/extract CLI execution, parsers, tournament budgeting and model quality remain separate tasks.

| Candidate | Identity | Ordering and source range |
| --- | --- | --- |
| Input line | `L<n>` | Physical 1-based line number, including skipped blank-line gaps. Verbatim offsets exclude the line terminator; raw output preserves original CRLF/LF and the unterminated final line. |
| Parsed span | `C<n>` | Assign once after sorting all original spans by `(byte_start, byte_end)`. Identical ranges collapse. Equal text at distinct positions remains distinct. |

IDs contain a canonical positive decimal integer without leading zeros. `none` and `uncertain` remain reserved labels. JSON candidates contain `id`, `verbatim` and the existing source `location` shape (`source`, `byte_start`, `byte_end`, `line_start`, `line_end`). Byte ranges are zero-based with an exclusive end; line positions are 1-based and inclusive. Empty physical lines can be retained explicitly. Parser matches must be nonempty valid UTF-8 spans.

Parsers operating on chunks first map local offsets into the original document. Collect and deduplicate those original spans, then assign global IDs. A chunk view borrows candidates without renumbering or changing offsets. Overlap can show an existing candidate repeatedly but does not create another identity. A span crossing a chunk boundary stays in the global table and cannot become a partial candidate. Line views require the complete raw line, including its terminator. Views refuse candidates from a different document snapshot; caller-supplied path metadata does not read or reopen a file.

Tests cover CRLF, skipped/retained empty lines, Unicode byte offsets, final lines, equal-text distinct spans, cross-line ranges, malformed/out-of-bounds spans, typed ID round trips and every character-boundary interval of a Unicode document. Mapping all overlapping intervals back in reverse order reconstructs exactly the original candidate JSON. These are direct core contracts, not model or retrieval-quality evidence.
