import importlib.util
import io
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

spec=importlib.util.spec_from_file_location('deploy',Path(__file__).parents[1]/'deploy.py')
deploy=importlib.util.module_from_spec(spec)
spec.loader.exec_module(deploy)

class DeploymentTests(unittest.TestCase):
    def bundle(self,path,tamper=False,link=False):
        rid='11111111-1111-4111-8111-111111111111'
        files={'release.json':json.dumps({'format_version':1,'release_id':rid}).encode(),
               'compatibility.json':json.dumps({'module_abi':1,'release_id':rid}).encode(),
               'nginx.conf.example':b'# config', 'README.txt':b'example'}
        manifest=''.join(hashlib.sha256(data).hexdigest()+'  '+name+'\n' for name,data in files.items())
        if tamper: files['release.json']=b'corrupt'
        files['SHA256SUMS']=manifest.encode()
        with tarfile.open(path,'w:gz') as tar:
            for name,data in files.items():
                info=tarfile.TarInfo(name);info.size=len(data)
                if link and name=='README.txt':info.type=tarfile.SYMTYPE;info.linkname='/etc/passwd';info.size=0
                tar.addfile(info,io.BytesIO(data))
    def test_archive_integrity_and_links(self):
        for tamper,link,valid in [(False,False,True),(True,False,False),(False,True,False)]:
            with tempfile.TemporaryDirectory() as d:
                root=Path(d);archive=root/'bundle.tar.gz';out=root/'out';out.mkdir();self.bundle(archive,tamper,link)
                if valid:self.assertEqual(deploy.unpack(archive,out),'11111111-1111-4111-8111-111111111111')
                else:
                    with self.assertRaises(ValueError):deploy.unpack(archive,out)
    def test_failed_activation_restores_previous(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);deploy.link(root,'current','releases/old')
            args=SimpleNamespace()
            with patch.object(deploy,'nginx',side_effect=[subprocess.CalledProcessError(1,['nginx','-t']),None,None]):
                with self.assertRaises(subprocess.CalledProcessError):deploy.activate(args,root,'releases/new')
            self.assertEqual((root/'current').readlink(),Path('releases/old'))
    def test_successful_activation_tracks_previous(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);deploy.link(root,'current','releases/old')
            with patch.object(deploy,'nginx') as nginx:
                deploy.activate(SimpleNamespace(),root,'releases/new')
                self.assertEqual(nginx.call_count,2)
            self.assertEqual((root/'current').readlink(),Path('releases/new'))
            self.assertEqual((root/'previous').readlink(),Path('releases/old'))

if __name__=='__main__':unittest.main()
