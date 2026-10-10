from pathlib import Path
import unittest

from tools.package_release import archive_markdown


class PackageDocumentationTests(unittest.TestCase):
    def test_root_document_rebases_local_links_and_preserves_source_references(
        self,
    ) -> None:
        root = Path(__file__).resolve().parents[1]
        content = (
            "[grants](mcp-access.md#paths) "
            "[source](../crates/loupe-cli/src/args.rs#L1)\n"
            "```text\n[example](mcp-access.md)\n```\n"
        ).encode()
        actual = archive_markdown(
            content,
            "docs/mcp.md",
            "mcp.md",
            {"docs/mcp-access.md"},
            root,
            "https://github.com/jpetrucciani/loupe",
            "v0.0.1",
        )
        self.assertEqual(
            actual.decode(),
            "[grants](docs/mcp-access.md#paths) "
            "[source](https://github.com/jpetrucciani/loupe/blob/v0.0.1/"
            "crates/loupe-cli/src/args.rs#L1)\n"
            "```text\n[example](mcp-access.md)\n```\n",
        )
