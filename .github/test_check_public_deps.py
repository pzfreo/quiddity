"""Public API dependency guard accepts allowlisted crates and rejects step_io."""
import unittest
from check_public_deps import ALWAYS_ALLOWED, RUSTDOC_CMD, check_doc


def fixture(output_type):
    return {
        "root": 0,
        "includes_private": False,
        "external_crates": {
            "1": {"name": "std"},
            "20": {"name": "step_io"},
            "38": {"name": "serde_json"},
        },
        "paths": {
            "100": {"crate_id": 1, "path": ["std", "string", "String"], "kind": "struct"},
            "200": {"crate_id": 20, "path": ["step_io", "parser", "Attribute"], "kind": "enum"},
            "300": {"crate_id": 38, "path": ["serde_json", "value", "Value"], "kind": "enum"},
        },
        "index": {
            "0": {
                "id": 0, "crate_id": 0, "name": "haecceity", "visibility": "public",
                "inner": {"module": {"is_crate": True, "is_stripped": False, "items": [1]}},
            },
            "1": {
                "id": 1, "crate_id": 0, "name": "parse_attr", "visibility": "public",
                "inner": {
                    "function": {
                        "generics": {"params": [], "where_predicates": []},
                        "sig": {
                            "inputs": [["text", {"resolved_path": {"path": "String", "id": 100, "args": None}}]],
                            "output": output_type,
                        },
                    }
                },
            },
        },
    }


class PublicDepsGuard(unittest.TestCase):
    def test_allowlisted_crates_pass(self):
        doc = fixture({"resolved_path": {"path": "Value", "id": 300, "args": None}})
        self.assertEqual(check_doc(doc, ALWAYS_ALLOWED | {"serde_json"}), [])

    def test_step_io_in_public_signature_fails(self):
        doc = fixture({"resolved_path": {"path": "Attribute", "id": 200, "args": None}})
        self.assertEqual(
            check_doc(doc, ALWAYS_ALLOWED | {"serde_json"}),
            [("haecceity::parse_attr", "step_io", "step_io::parser::Attribute")],
        )

    def test_step_io_in_doc_hidden_public_signature_fails(self):
        self.assertIn("--document-hidden-items", RUSTDOC_CMD)
        doc = fixture({"resolved_path": {"path": "Attribute", "id": 200, "args": None}})
        doc["index"]["1"]["attrs"] = [{"other": "#[doc(hidden)]"}]
        self.assertEqual(
            check_doc(doc, ALWAYS_ALLOWED | {"serde_json"}),
            [("haecceity::parse_attr", "step_io", "step_io::parser::Attribute")],
        )
