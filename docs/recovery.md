# General run recovery journal

`--failed FILE` creates a new durable JSONL journal for an inference command. Repeat the original command, semantic flags and original input with `--resume FILE` to continue an interrupted run. The flags are mutually exclusive. An existing artifact is never overwritten by `--failed`, and `--resume` requires an existing artifact. `tokens` and `--dry-run` reject both flags with exit 64.

```sh
loupe --profile server keep 'Is checkout unavailable?' --json --failed run.jsonl < input.txt > first.jsonl
loupe --profile server keep 'Is checkout unavailable?' --json --resume run.jsonl --budget 20000 < input.txt > rest.jsonl
```

The journal includes complete input data, request plans and actual responses. Its mode is 0600; store it as private data. The generic journal is separate from the transactional route-file protocol in [route-recovery.md](route-recovery.md).

External uncertainty fallback adds a separate synced frame after the raw completed result and before output. Run identity pins its command, timeout and failure policy. Completed fallback results, including fatal failures, replay without rerunning the command or changing the model answer. A crash between the external effect and its frame can repeat that work. [fallback.md](fallback.md) defines the result/probability and process boundaries.

## Resumption

Run identity pins the command, question insertion order and effective instructions/options, input sources/readers/context, endpoint/model and available discovered identity, tokenizer/encoder, packing and limits, split/truncate/focus settings, output format, threshold/uncertainty/error policy, capture destination and route destinations. Credentials are excluded. A semantic mismatch exits 65. Concurrency, deadline and budget can change; each invocation has its own token budget. Missing authoritative model identity still disables response caching. Recovery preserves responses from this specific run, but cannot detect an unadvertised server build or weights change for new pending requests.

Resume rereads and compares all previously admitted units before releasing saved work. Comparison includes raw bytes, parsed/context state, physical indices, source locations and headers. When the original run reached EOF, raw source fingerprints also validate skipped blanks, headers and source order before any saved output or new inference. Shortened, changed or extended recorded input exits 65. An incomplete stdin source requires the original stream again; unread tail bytes were never available to persist.

Already acknowledged emitted/omitted units are skipped. Completed-but-unemitted units reuse their actual responses, answer bindings, original request packing and split/truncate/focus provenance. Restore validates response shape, question/answer order, source binding and freshly recomputed token counts. This is continuation of a recorded run; it does not turn server's alias into an eligible cache identity. Restored results preserve their original `cached` value and response usage, with zero new logical evaluations, attempts or token charges.

Failed/pending units without a completed result are replanned under the same declared semantics. Their new grouping can differ around saved/skipped units. A failed split/focus unit repeats its phases; partial phase responses remain provenance, without becoming a completed answer. Every newly submitted attempt, including a retry, is charged normally.

Sort/count retain completed results until output is flushed. Sort records per-unit acknowledgements as its ordered output is written; count records its aggregate receipt after the final flush. A finished run validates the original input, emits nothing, makes no inference request and returns the saved normal exit code, including `is` yes/no/uncertain and empty-output exits.

For `route`, `.loupe-route.jsonl` is the reserved manifest filename. `--resume DIR/.loupe-route.jsonl` uses route's file protocol. A different resume filename selects general recovery and must live outside `--out`; it validates the same original input and reconciles route's journal before skipping acknowledged units. All resume modes reject process destinations, whose external side effects cannot be replayed safely.

## Journal protocol

The JSONL journal is the portable, authoritative artifact. It contains complete original units and opaque request/result provenance supplied by the caller. Every frame includes format version 1, a consecutive sequence number, the preceding checksum, a typed record and a BLAKE3 checksum over the version/sequence/previous-checksum/record tuple. Removing, reordering or changing a complete frame invalidates the chain. Unknown fields and invalid state transitions are rejected.

| Record | Contract |
| --- | --- |
| `begin` | Effective semantic run identity; first frame only. Credentials must be excluded by the caller. |
| `admit` | Complete original unit, including bytes, subject, state, header and source location. Indices increase, preserving physical gaps. Persist before enqueueing. |
| `plan` | Increasing indices and exact request/count/packing/view provenance. Persist before submitting inference; one plan can bind several joint units. |
| `ready` | Original index and completed result, including the actual response and all derived provenance. Persist before returning to ordered buffering. |
| `ready_many` | Increasing indices and completed results from one job, saved together in one frame. A torn frame restores none of its members; a complete frame restores all. |
| `failed` | Optional original index plus failure code/message. A missing index reports a reader/global failure without inventing a unit. |
| `handled` | Emitted or omitted decision, persisted after the destination write/flush or omission is complete. Repeating the same acknowledgement is a no-op. |
| `input_end` | Complete-source fingerprint, supplied after successful original-input EOF. No further admissions are accepted. |
| `finish` | Normal exit and emitted count, persisted after final output flush. Requires input EOF and outcomes for every admitted unit; pending work cannot be silently acknowledged. A final aggregate can acknowledge its saved results together. |

Ready units are independent of output acknowledgement, so completion ahead of a failed prefix remains replayable. A failed unit can retain both its prior completed result and a later policy/output failure. Pending units have an admission and optional plan but no completed result. The journal retains these distinctions without fabricated answers. Finished runs have a final receipt, and no new records may extend them.

The index maps original indices to journal offsets and acknowledgement state. It lives in an anonymous file and is rebuilt by streaming the journal on every open. Its configured page cache is 8 MiB; unit/request/result payloads remain in the journal rather than an in-memory map. Memory includes the largest current frame, run identity and index metadata/cache. Disk space grows with recorded admissions, provenance and results. Linux uses an unnamed temporary file; the Unix fallback exclusively creates and immediately unlinks a mode-0600 file through a held scratch-directory descriptor. No persistent index sidecar is needed for replay.

Creation exclusively opens a new regular, single-link file in mode 0600. The final component uses descriptor-relative NOFOLLOW/nonblocking opens, and a nonblocking file-inode writer lock excludes concurrent owners. Caller-provided protected input/output device/inode pairs are checked before any journal writes or repairs. Creation syncs the journal and its held parent directory. Resume requires an existing artifact, validates the complete chain and expected run identity before truncating a final incomplete frame, and syncs that repair. Complete corrupt or invalid frames are retained and fail with exit 74; a changed/missing run identity is exit 65. A write, sync or index-update failure poisons the writer until it is reopened.

The disposable index uses nondurable transactions, because every accepted update follows a synced authoritative frame and the index is rebuilt after a crash. This use differs from the response cache, whose own persisted index is authoritative. Redb's durability modes are documented in its [official reference](https://docs.rs/redb/4.3.0/redb/enum.Durability.html).

The CLI persists admissions before channel enqueueing, exact request plans before submission and completed results before returning to ordered buffering. Successful members of one completed job are saved together before failure records. Per-unit destinations acknowledge after successful flush or omission; finite aggregates retain their saved results until output is acknowledged. Cancellation stops new admissions without waiting for an input thread blocked on an incomplete pipe. The HTTP admission queue is FIFO, so later split chunks cannot repeatedly take capacity ahead of an already waiting unrelated unit; cancelling a waiter releases its queue position.

Generic stdout and ordinary uncertainty files have no shared transaction with the journal. A crash after a successful write but before durable acknowledgement can cause repeated bytes on resume; a failed partial write cannot retract bytes already consumed downstream. Keep previously acknowledged output if a complete result is needed, since resume emits only the remaining work. Transactional route-file writes retain their stronger recovery protocol. External pipe/process delivery has no exactly-once guarantee.
