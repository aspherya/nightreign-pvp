import importlib.util
import json
import re
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("patch_talk", HERE / "patch_talk.py")
patcher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(patcher)


class TrainingMenuTests(unittest.TestCase):
    def setUp(self):
        self.original = "def t607001000_x2():\n    assert t607001000_x3()\n    return 0\n\ndef t607001000_x3():\n    OpenSparringEquipmentMenu()\n    return 0\n"
        self.result = patcher.patch(self.original)

    def test_retains_original_interaction_and_rejects_repatch(self):
        self.assertIn(self.original.split("\n\ndef")[0], self.result)
        self.assertEqual(self.result.count("OpenSparringEquipmentMenu()"), 1)
        self.assertIn("def t607001000_x903():", self.result)
        with self.assertRaises(ValueError):
            patcher.patch(self.result)

    def test_all_ten_slots_have_save_load_and_cancel_paths(self):
        for slot in range(10):
            self.assertIn(f"SetEventFlag({patcher.SAVE + slot}, FlagState.On)", self.result)
            self.assertIn(f"SetEventFlag({patcher.LOAD + slot}, FlagState.On)", self.result)
            self.assertIn(f"def t607001000_x{920 + slot}():", self.result)
            self.assertIn(f"def t607001000_x{940 + slot}():", self.result)
        self.assertTrue(re.search(r"if GetTalkListEntryResult\(\) != 1:\n\s+return 0", self.result))
        self.assertNotIn("OpenChangeCharacterMenu()", self.result)
        self.assertNotIn("GetChrHero()", self.result)
        self.assertIn("GetCurrentStateElapsedTime() > 15", self.result)

    def test_menu_texts_and_protocol_fit_reserved_ranges(self):
        entries = json.loads((HERE.parents[1] / "gamedata/fmg-entries.json").read_text())["EventTextForTalk_dlc01"]
        ids = re.findall(r"AddTalkListDataAlt(?:If)?\(.*?, (991\d+), -1", self.result)
        ids += re.findall(r"OpenGenericDialog\(DialogBoxType.CenterBottom1, (\d+)", self.result)
        self.assertTrue(ids)
        for text_id in ids:
            self.assertIn(text_id, entries)
        for slot in range(10):
            self.assertEqual(entries[str(patcher.TEXT + 30 + slot)], f"Slot {slot + 1}: Saved")


if __name__ == "__main__":
    unittest.main()
