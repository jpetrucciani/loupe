"""Document actual Serde declarations from the compiler's Rustdoc JSON AST."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from dataclasses import dataclass
from pathlib import Path

type Json = str | int | float | bool | None | list[Json] | dict[str, Json]
type Object = dict[str, Json]


def object_value(value: Json) -> Object:
    if not isinstance(value, dict):
        raise TypeError(f"expected a Rustdoc object, got {type(value).__name__}")
    return value


def array(value: Json) -> list[Json]:
    if not isinstance(value, list):
        raise TypeError(f"expected a Rustdoc array, got {type(value).__name__}")
    return value


def string(value: Json) -> str:
    if not isinstance(value, str):
        raise TypeError(f"expected a Rustdoc string, got {type(value).__name__}")
    return value


def attributes(item: Object) -> list[str]:
    values = []
    for attribute in array(item["attrs"]):
        text = (
            attribute
            if isinstance(attribute, str)
            else object_value(attribute).get("other")
        )
        if isinstance(text, str) and text.startswith("#[serde("):
            values.append(text)
    return values


def setting(item: Object, key: str) -> str | None:
    for attribute in attributes(item):
        match = re.search(rf'\b{key}\s*=\s*"([^"]*)"', attribute)
        if match:
            return match.group(1)
    return None


def anchor(name: str) -> str:
    return name.replace("::", "-").replace("_", "-").lower()


def cell(text: str) -> str:
    return text.replace("|", "&#124;").replace("\n", " ")


def source(item: Object) -> str:
    span = object_value(item["span"])
    file = string(span["filename"])
    line = array(span["begin"])[0]
    return f"[{file}:{line}](../{file}#L{line})"


@dataclass
class Document:
    name: str
    index: dict[str, Object]
    paths: dict[str, Object]
    local_paths: dict[str, str]
    serialized: set[str]
    format_version: int

    @classmethod
    def load(cls, file: Path) -> Document:
        data: Object = json.loads(file.read_text())
        if data["format_version"] != 61:
            raise ValueError(
                "Rustdoc format changed; review its AST before updating the catalog reader"
            )
        index = {
            key: object_value(value)
            for key, value in object_value(data["index"]).items()
        }
        paths = {
            key: object_value(value)
            for key, value in object_value(data["paths"]).items()
        }
        local_paths: dict[str, str] = {}

        def visit(identifier: str, parent: str) -> None:
            item = index[identifier]
            name = string(item["name"])
            path = f"{parent}::{name}" if parent else name
            local_paths[identifier] = path
            module = object_value(item["inner"]).get("module")
            if module is not None:
                for child in array(object_value(module)["items"]):
                    child_id = str(child)
                    child_item = index[child_id]
                    if child_item["name"] is not None:
                        visit(child_id, path)

        visit(str(data["root"]), "")
        serialized: set[str] = set()
        for item in index.values():
            implementation = object_value(item["inner"]).get("impl")
            if item["crate_id"] != 0 or implementation is None:
                continue
            implementation = object_value(implementation)
            trait = implementation["trait"]
            if (
                trait is None
                or string(object_value(trait)["path"]).split("::")[-1] != "Serialize"
            ):
                continue
            target = object_value(implementation["for"]).get("resolved_path")
            if target is None:
                raise ValueError(
                    "a new Serialize implementation needs explicit catalog handling"
                )
            identifier = str(object_value(target)["id"])
            if index[identifier]["crate_id"] == 0:
                if "automatically_derived" not in array(item["attrs"]):
                    raise ValueError(
                        f"custom Serialize implementation needs a wire-shape review: {identifier}"
                    )
                serialized.add(identifier)
        return cls(
            string(index[str(data["root"])]["name"]),
            index,
            paths,
            local_paths,
            serialized,
            61,
        )

    def path(self, identifier: str, fallback: str) -> str:
        if identifier in self.local_paths:
            return self.local_paths[identifier]
        if identifier in self.paths:
            return "::".join(
                string(part) for part in array(self.paths[identifier]["path"])
            )
        raise ValueError(f"unresolved Rustdoc path {identifier}: {fallback}")


@dataclass
class Catalog:
    documents: list[Document]

    def linked(self, path: str) -> str:
        if any(
            path == doc.local_paths[identifier]
            for doc in self.documents
            for identifier in doc.serialized
        ):
            return f"[{path}](#{anchor(path)})"
        return f"`{path}`"

    def type(self, value: Object, doc: Document) -> str:
        if "primitive" in value:
            kind = string(value["primitive"])
            if kind in {"str", "char"}:
                return "string"
            if kind == "bool":
                return "boolean"
            if kind.startswith(("i", "u")):
                return f"integer ({kind})"
            if kind.startswith("f"):
                return f"number ({kind})"
            raise ValueError(f"new primitive needs a JSON mapping: {kind}")
        if "generic" in value:
            return f"type parameter `{string(value['generic'])}`"
        if "borrowed_ref" in value:
            return self.type(
                object_value(object_value(value["borrowed_ref"])["type"]), doc
            )
        if "slice" in value:
            return f"array of {self.type(object_value(value['slice']), doc)}"
        if "array" in value:
            details = object_value(value["array"])
            return f"array[{details['len']}] of {self.type(object_value(details['type']), doc)}"
        if "tuple" in value:
            values = array(value["tuple"])
            return (
                "null"
                if not values
                else "tuple array ("
                + ", ".join(self.type(object_value(part), doc) for part in values)
                + ")"
            )
        if "resolved_path" not in value:
            raise ValueError(f"new field type needs a JSON mapping: {value}")
        details = object_value(value["resolved_path"])
        path = doc.path(str(details["id"]), string(details["path"]))
        name = path.split("::")[-1]
        args: list[Json] = []
        if details["args"] is not None:
            args = array(
                object_value(object_value(details["args"])["angle_bracketed"])["args"]
            )
        types = [
            self.type(object_value(object_value(arg)["type"]), doc)
            for arg in args
            if "type" in object_value(arg)
        ]
        if name == "Option":
            return types[0] + " or null"
        if name in {"Box", "Arc", "Rc", "Cow"}:
            return types[-1]
        if name in {"Vec", "HashSet", "BTreeSet"}:
            return "array of " + types[0]
        if name in {"HashMap", "BTreeMap", "IndexMap"}:
            return f"object mapping {types[0]} keys to {types[1]}"
        if name in {"String", "PathBuf", "Path"}:
            return "string" + (" (UTF-8 filesystem path)" if name != "String" else "")
        if name.startswith("NonZero"):
            return f"nonzero integer ({name})"
        if name == "Value" and "serde_json" in path:
            return "arbitrary JSON value"
        if name in {"Range", "RangeInclusive"} and path.startswith(
            ("core::ops::", "std::ops::")
        ):
            return f"object {{start, end}}, each {types[0]}"
        if name == "Duration":
            return "object {secs: unsigned integer, nanos: unsigned integer}"
        return self.linked(path) + ("<" + ", ".join(types) + ">" if types else "")

    def fields(self, identifiers: list[Json], doc: Document) -> list[str]:
        rows = [
            "| JSON key / tuple position | Value | Serde attributes |",
            "| --- | --- | --- |",
        ]
        for identifier in identifiers:
            if identifier is None:
                raise ValueError(
                    "stripped field in a serialized type; require --document-private-items"
                )
            item = doc.index[str(identifier)]
            name = setting(item, "rename") or string(item["name"])
            if setting(item, "serialize_with") or setting(item, "with"):
                raise ValueError(
                    f"custom field serializer needs a wire-shape review: {name}"
                )
            if any(
                re.search(r"\b(skip|skip_serializing)\b", attribute)
                for attribute in attributes(item)
            ):
                name += " (storage only, skipped)"
            attrs = (
                "; ".join(f"`{attribute}`" for attribute in attributes(item))
                or "Always emitted; no field-level overrides."
            )
            rows.append(
                f"| `{cell(name)}` | {cell(self.type(object_value(object_value(item['inner'])['struct_field']), doc))} | {cell(attrs)} |"
            )
        return rows

    def definition(self, identifier: str, doc: Document) -> list[str]:
        item = doc.index[identifier]
        path = doc.local_paths[identifier]
        lines = [
            f'<a id="{anchor(path)}"></a>',
            f"## `{path}`",
            "",
            f"Source: {source(item)}.",
            "",
        ]
        attrs = attributes(item)
        if attrs:
            lines.extend(
                [
                    "Container attributes: "
                    + "; ".join(f"`{attribute}`" for attribute in attrs)
                    + ".",
                    "",
                ]
            )
        if item["docs"] is not None:
            lines.extend([string(item["docs"]), ""])
        convert = setting(item, "into")
        if convert:
            lines.extend(
                [
                    f"JSON serialization converts the value into `{convert}`. The fields below describe its Rust storage, rather than JSON object keys.",
                    "",
                ]
            )
        inner = object_value(item["inner"])
        if "struct" in inner:
            kind = object_value(inner["struct"])["kind"]
            if kind == "unit":
                lines.append("JSON null (unit struct).")
            else:
                kind = object_value(kind)
                if "plain" in kind:
                    details = object_value(kind["plain"])
                    if details["has_stripped_fields"]:
                        raise ValueError(f"stripped fields: {path}")
                    fields = array(details["fields"])
                else:
                    fields = array(kind["tuple"])
                    if not convert:
                        lines.extend(
                            [
                                "A tuple array, unless the container's `transparent` attribute emits its single inner value.",
                                "",
                            ]
                        )
                lines.extend(self.fields(fields, doc))
        elif "enum" in inner:
            if object_value(inner["enum"])["has_stripped_variants"]:
                raise ValueError(f"stripped enum variants: {path}")
            tag, content = setting(item, "tag"), setting(item, "content")
            untagged = any("untagged" in attribute for attribute in attrs)
            if tag:
                lines.extend(
                    [
                        f"Discriminator key: `{tag}`."
                        + (
                            f" Payload key: `{content}`."
                            if content
                            else " Variant fields merge into the same object."
                        ),
                        "",
                    ]
                )
            elif untagged:
                lines.extend(
                    [
                        "No discriminator or variant wrapper; each variant emits its payload directly.",
                        "",
                    ]
                )
            else:
                lines.extend(
                    [
                        "Default external tagging: unit variants are strings; variants with payloads are one-key objects.",
                        "",
                    ]
                )
            for variant in array(object_value(inner["enum"])["variants"]):
                data = doc.index[str(variant)]
                name = setting(data, "rename")
                if name is None:
                    name = string(data["name"])
                    case = setting(item, "rename_all")
                    if case in {"snake_case", "kebab-case"}:
                        name = re.sub(
                            r"(?<!^)(?=[A-Z])",
                            "_" if case == "snake_case" else "-",
                            name,
                        ).lower()
                    elif case == "lowercase":
                        name = name.lower()
                    elif case is not None:
                        raise ValueError(f"new enum rename rule needs review: {case}")
                lines.extend([f"### `{name}`", ""])
                kind = object_value(object_value(data["inner"])["variant"])["kind"]
                if kind == "plain":
                    lines.append("No payload.")
                else:
                    kind = object_value(kind)
                    fields = (
                        array(object_value(kind["struct"])["fields"])
                        if "struct" in kind
                        else array(kind["tuple"])
                    )
                    lines.extend(self.fields(fields, doc))
                lines.append("")
        else:
            raise ValueError(f"new serializable declaration needs review: {path}")
        return lines + [""]

    def render(self) -> str:
        definitions = sorted(
            (doc.local_paths[identifier], identifier, doc)
            for doc in self.documents
            for identifier in doc.serialized
        )
        lines = [
            "# JSON type field reference",
            "",
            "Generated from both crates' actual all-feature Rustdoc JSON ASTs, including private items. This catalog covers every local `Serialize` implementation, with real field types and Serde attributes. It is a declaration reference, not a JSON Schema validator or a stable Rust library API. [JSON interfaces](json-interfaces.md) maps command outputs and anonymous provenance objects onto these definitions.",
            "",
            f"The current catalog contains {len(definitions)} serialized declarations. Rustdoc format {self.documents[0].format_version} is explicitly checked; unsupported AST/type/serialization forms fail generation rather than silently disappearing.",
            "",
            "An `Option` without `skip_serializing_if` emits JSON null when absent. A skip predicate omits its key when true. `default` governs deserialization and does not by itself omit output. `alias` accepts an input name; it does not change the emitted name. `flatten` merges the referenced fields into the parent object. Container `tag`, `content`, `rename_all`, `transparent`, `into` and field attributes determine the declared wire layout. Arbitrary JSON fields require the command-specific semantics linked from the interface map; their content is not inferred from a sample.",
            "",
            "Rust integer bounds remain applicable; `usize`/`isize` use the native platform width. JSON object map keys are strings, including rendered numeric indices. Filesystem paths require UTF-8 for JSON serialization. Probability validation, supported normalization domains, request/answer consistency and source bounds are enforced in the referenced implementations and documented interface contracts.",
            "",
            "## Definitions",
            "",
        ]
        lines.extend(f"- [{path}](#{anchor(path)})" for path, _, _ in definitions)
        lines.append("")
        for _, identifier, doc in definitions:
            lines.extend(self.definition(identifier, doc))
        return "\n".join(lines).rstrip() + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--core", type=Path, default=Path("target/doc/loupe_core.json"))
    parser.add_argument("--cli", type=Path, default=Path("target/doc/loupe.json"))
    parser.add_argument("--out", type=Path, default=Path("docs/json-types.md"))
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    documents = [Document.load(args.core), Document.load(args.cli)]
    generated = Catalog(documents).render()
    if args.check:
        if args.out.read_text() != generated:
            raise ValueError(
                f"serialized declarations changed: regenerate and review {args.out}"
            )
    else:
        args.out.write_text(generated)
    print(
        f"{sum(len(doc.serialized) for doc in documents)} declarations; sha256={hashlib.sha256(generated.encode()).hexdigest()}"
    )


if __name__ == "__main__":
    main()
