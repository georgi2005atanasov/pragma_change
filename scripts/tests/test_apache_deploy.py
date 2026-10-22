import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
from uuid import uuid4

spec = importlib.util.spec_from_file_location("apache_deploy", Path(__file__).parents[1] / "deploy-apache.py")
deploy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(deploy)


class ApacheDeploymentTests(unittest.TestCase):
    def bundle(self, path, tamper=False, link=False, target="apache"):
        identifier = str(uuid4())
        files = {"release.json": json.dumps({"format_version":1,"release_id":identifier}).encode(), "compatibility.json":json.dumps({"format_version":1,"module_abi":1,"proxy":target,"target":"x86_64-unknown-linux-gnu","release_id":identifier}).encode(), "apache.conf.example":b"config", "README.txt":b"readme"}
        files["SHA256SUMS"] = "".join(hashlib.sha256(data).hexdigest()+"  "+name+"\n" for name,data in files.items()).encode()
        if tamper:
            files["release.json"] = b"corrupt"
        with tarfile.open(path, "w:gz") as tar:
            for name,data in files.items():
                entry = tarfile.TarInfo(name)
                entry.size = len(data)
                if link and name == "README.txt":
                    entry.type = tarfile.SYMTYPE; entry.linkname = "/etc/passwd"; entry.size = 0
                tar.addfile(entry, io.BytesIO(data))
        return identifier

    def test_archive_integrity_and_target(self):
        for tamper,link,target,valid in [(False,False,"apache",True),(True,False,"apache",False),(False,True,"apache",False),(False,False,"nginx",False)]:
            with self.subTest(tamper=tamper, link=link, target=target), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp); out = root / "out"; out.mkdir()
                identifier = self.bundle(root / "bundle.tgz", tamper, link, target)
                if valid:
                    self.assertEqual(deploy.unpack(root / "bundle.tgz", out), identifier)
                else:
                    with self.assertRaises(ValueError):
                        deploy.unpack(root / "bundle.tgz", out)

    def prepare(self, root):
        old, new = str(uuid4()), str(uuid4())
        for identifier in (old,new):
            directory = root / "releases" / identifier; directory.mkdir(parents=True)
            (directory / "release.json").write_text(json.dumps({"release_id":identifier}))
        deploy.link(root, "current", "releases/"+old)
        return old,new,SimpleNamespace(start=False,status_url="http://127.0.0.1:19001/pragmachange-status",timeout=1)

    def test_failed_validation_or_reload_restores_previous(self):
        for failure in (0,1):
            with tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp); old,new,args = self.prepare(root)
                effects = [None]*failure + [subprocess.CalledProcessError(1,["apache2"])] + [None,None]
                with patch.object(deploy,"apache",side_effect=effects), patch.object(deploy,"wait_for") as wait:
                    with self.assertRaises(subprocess.CalledProcessError):
                        deploy.activate(args,root,"releases/"+new)
                    wait.assert_called_once_with(args.status_url,old,args.timeout)
                self.assertEqual((root/"current").readlink(),Path("releases/"+old))

    def test_wrong_loaded_identity_rolls_back(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); old,new,args=self.prepare(root)
            with patch.object(deploy,"apache"), patch.object(deploy,"wait_for",side_effect=[RuntimeError("wrong release"),{"release_id":old}]):
                with self.assertRaisesRegex(RuntimeError,"wrong release"):
                    deploy.activate(args,root,"releases/"+new)
            self.assertEqual((root/"current").readlink(),Path("releases/"+old))
            self.assertFalse((root/"previous").exists())

    def test_success_tracks_previous_and_path_escape_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); old,new,args=self.prepare(root)
            with patch.object(deploy,"apache"),patch.object(deploy,"wait_for",return_value={"release_id":new}):
                self.assertEqual(deploy.activate(args,root,"releases/"+new)["release_id"],new)
            self.assertEqual((root/"previous").readlink(),Path("releases/"+old))
            for path in ("../elsewhere", "/tmp/release", "releases/../../etc"):
                with self.assertRaises(ValueError):deploy.installed(root,path)

    def test_status_only_literal_loopback_without_redirects(self):
        for url in ("https://127.0.0.1/status","http://localhost/status","http://192.0.2.1/status","http://user@127.0.0.1/status"):
            with self.assertRaises(ValueError):deploy.status(url)
        with self.assertRaises(ValueError):
            deploy.NoRedirect().redirect_request(None,None,302,"",{},"http://192.0.2.1/")


if __name__ == "__main__":
    unittest.main()
