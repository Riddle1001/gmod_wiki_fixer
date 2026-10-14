"""Exercise the real CLI against temporary Git repositories and local HTTP fixtures.

No live wiki or working-tree source changes. Logs and fixtures are retained for review.
Run after cargo build; optional --repo exercises the real source snapshot without HTTP.
"""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import unquote, urlparse


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    project = Path(__file__).resolve().parent.parent
    parser.add_argument("--binary", type=Path, default=project / "target/debug/gmod_wiki_fixer.exe")
    parser.add_argument("--repo", type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    root = Path(tempfile.mkdtemp(prefix="gmod-cli-regression-"))
    log_path = root / "verification.jsonl"
    start = time.monotonic()
    failures = []
    lock = threading.Lock()
    log = log_path.open("w", encoding="utf-8")

    def record(event, **data):
        with lock:
            log.write(json.dumps({"elapsed": time.monotonic() - start, "event": event, **data}) + "\n")
            log.flush()

    def check(name, condition, **detail):
        record("assert", name=name, passed=bool(condition), **detail)
        if not condition:
            failures.append(name)

    def run(name, command, cwd=root):
        result = subprocess.run(command, cwd=cwd, capture_output=True, text=True,
                                encoding="utf-8", errors="replace", timeout=180)
        record("process", name=name, command=[str(x) for x in command], code=result.returncode,
               stdout=result.stdout, stderr=result.stderr)
        return result

    record("CANARY", message="Capture active; all cases first-pass, no retries")
    repo = root / "repo"
    lua = repo / "garrysmod/lua"
    lua.mkdir(parents=True)
    source = lua / "probe.lua"
    original = "function Probe() end\nfunction probe() end\n"
    source.write_text(original, encoding="utf-8")
    (lua / "client").mkdir()
    (lua / "server").mkdir()
    (lua / "client/realm.lua").write_text("function RealmProbe() end\n", encoding="utf-8")
    (lua / "server/realm.lua").write_text("function RealmProbe() end\n", encoding="utf-8")

    (lua / "controls.lua").write_text('''local P = {}
function P:OnStartDragging() end
vgui.Register("ControlProbeChild", P, "Panel")
vgui.Register("ControlProbeEmpty", {}, "Panel")
derma.DefineControl("ControlProbeKnown", "Known", {}, "Panel")
vgui.Register("ControlProbeFailure", {}, "Panel")
local vgui = { Register = function() end }
vgui.Register("ControlProbePrivate", {}, "Panel")
''', encoding="utf-8")

    (lua / "registered_meta.lua").write_text('''local COLOR = {}
RegisterMetaTable("Color", COLOR)
function COLOR:Copy() return self end
local Other = {}
function Other:Copy() end
local RegisterMetaTable = function() end
RegisterMetaTable("PrivateMeta", Other)
''', encoding="utf-8")

    entity = repo / "garrysmod/gamemodes/base/entities/entities/base_ai"
    entity.mkdir(parents=True)
    (entity / "schedules.lua").write_text("function ENT:DoingEngineSchedule() return self.bDoingEngineSchedule end\n", encoding="utf-8")
    (entity / "ambiguous.lua").write_text("function ENT:Ambiguous() end\nfunction ENT:Ambiguous(x) end\n", encoding="utf-8")
    (lua / "installed.lua").write_text('''local meta = FindMetaTable("Entity")
function meta:InstallDataTable()
    self.NetworkVar = function(ent, typename, index, name, other_data) end
    self.UndocumentedInstanceCallback = function(ent) end
    self.NoReceiver = function() return {} end
    local self = {}
    self.ShadowedCallback = function() end
end
function meta:ExplicitSelf(self)
    self.ParameterCallback = function() end
end
function meta:First() self.AmbiguousInstalled = function(ent) end end
function meta:Second() self.AmbiguousInstalled = function(ent, value) end end
function Unknown:Install() self.UnknownCallback = function() end end
local native = FindMetaTable("Player")
native.GetName = native.Nick
native.OtherAlias = native.EngineMethod
native.ChainedAlias = native.OtherAlias
native = {}
native.ReboundAlias = native.Nick
function GM:HookProbe(ignored) local observed = 1 end
function OrdinaryReturnProbe() local observed = 1 end
util.AnyNativeAlias = util.TraceLine
local public_library = util
public_library.AnotherNativeAlias = public_library.AnyNativeAlias
''', encoding="utf-8")
    player_class = repo / "garrysmod/gamemodes/base/gamemode/player_class"
    player_class.mkdir(parents=True)
    (player_class / "player_default.lua").write_text('''local PLAYER = {}
function PLAYER:Spawn() end
function PLAYER:UndocumentedHook() end
player_manager.RegisterClass("player_default", PLAYER, nil)
''', encoding="utf-8")
    nextbot = repo / "garrysmod/gamemodes/base/entities/entities/base_nextbot"
    nextbot.mkdir(parents=True)
    (nextbot / "sv_nextbot.lua").write_text("function ENT:BehaveStart() end\n", encoding="utf-8")
    modules = lua / "includes/modules"
    modules.mkdir(parents=True)
    (modules / "ai_task.lua").write_text('''local setmetatable = setmetatable
module("ai_task")
local Task = {}
Task.__index = Task
function Task:Start(npc) end
function Task:UndocumentedFactoryMethod() end
function New()
    local object = {}
    setmetatable(object, Task)
    return object
end
''', encoding="utf-8")
    (modules / "ai_schedule.lua").write_text('''local setmetatable = setmetatable
module("ai_schedule")
local Schedule = {}
Schedule.__index = Schedule
function Schedule:Init() end
function New()
    local object = {}
    if UNKNOWN then setmetatable(object, Schedule) end
    return object
end
''', encoding="utf-8")
    foreign = repo / "garrysmod/gamemodes/other/entities/entities/foreign"
    foreign.mkdir(parents=True)
    (foreign / "schedules.lua").write_text("function ENT:DoingEngineSchedule(wrong, owner) end\n", encoding="utf-8")
    weapons = repo / "garrysmod/gamemodes/base/entities/weapons/test"
    weapons.mkdir(parents=True)
    (weapons / "shared.lua").write_text("function SWEP:WeaponProbe() end\n", encoding="utf-8")
    (lua / "menu").mkdir()
    (lua / "menu/menu.lua").write_text('function MenuPathOnly() end\ninclude("outside_menu.lua")\n', encoding="utf-8")
    (lua / "outside_menu.lua").write_text('function LoadedMenuOnly() end\n', encoding="utf-8")
    (lua / "policy.lua").write_text('''if MENU_DLL then function MenuGuardOnly() end end
if CLIENT then function SharedMenuProbe() end end
function __call() end
FixtureLibrary = {}
function FixtureLibrary.__helper() end
local META = {}
RegisterMetaTable("FixtureClass", META)
function META:__tostring() return "fixture" end
function META:Ordinary() end
function META:__add(other) return self end
''', encoding="utf-8")

    def git(*arguments):
        result = run("git", ["git", "-c", "user.name=CLI Fixture", "-c", "user.email=fixture@example.invalid",
                             "-c", f"core.hooksPath={root / 'no-hooks'}", *arguments], repo)
        if result.returncode:
            raise RuntimeError(result.stderr)
        return result.stdout.strip()

    git("init", "-q")
    git("remote", "add", "origin", "https://github.com/Facepunch/garrysmod.git")
    git("add", ".")
    git("commit", "-qm", "fixture")
    revision = git("rev-parse", "HEAD")
    state = {"index_status": 200, "version": 1, "requests": [], "pages": {}}

    def page(address, markup="", title=None):
        return {"address": address, "title": title or address, "markup": markup, "revisionId": 1}

    state["pages"] = {
        "Global.Probe": page("Global.Probe", "<function name='Probe'><file line='9'>lua/probe.lua</file></function>"),
        "Global.probe": page("Global.probe", "<function name='probe'><file line='2'>lua/probe.lua</file></function>"),
        "Review:A": page("Review:A", title="Colon page"),
        "Review_A": page("Review_A", title="Underscore page"),
        "Color:Copy": page("Color:Copy", "<function name='Copy' parent='Color' type='classfunc'><file line='99'>lua/registered_meta.lua</file></function>"),
        "ControlProbeKnown": page("ControlProbeKnown"),
        "ControlProbeFailure": 503,
        "PANEL:OnStartDragging": page("PANEL:OnStartDragging"),
        "Failure": 500,
        "Traversal": page("Traversal", "<function name='Outside'><file line='99'>../../outside.lua</file></function>"),
        "Oversized": page("Oversized", "<function name='Probe'><file line='18446744073709551615-L18446744073709551615'>lua/probe.lua</file></function>"),
        "Global.RealmProbe": page("Global.RealmProbe", "<function name='RealmProbe'><realm>Client</realm><file line='1'>lua/server/realm.lua</file></function>"),
    }

    state["pages"].update({
        "Player:GetName": page("Player:GetName", "<function name='GetName' parent='Player' type='classfunc'><file line='16'>lua/installed.lua</file><description>Returns the player name.</description><realm>Shared</realm><rets><ret type='string'/></rets></function>"),
        "Player:OtherAlias": page("Player:OtherAlias", "<function name='OtherAlias' parent='Player' type='classfunc'><file line='17'>lua/installed.lua</file></function>"),
        "Player:ChainedAlias": page("Player:ChainedAlias", "<function name='ChainedAlias' parent='Player' type='classfunc'><file line='18'>lua/installed.lua</file></function>"),
        "Player:ReboundAlias": page("Player:ReboundAlias", "<function name='ReboundAlias' parent='Player' type='classfunc'><file line='20'>lua/installed.lua</file></function>"),
        "GM:HookProbe": page("GM:HookProbe", "<function name='HookProbe' parent='GM' type='hook'><file line='21'>lua/installed.lua</file><description>Runs before drawing.</description><realm>Client</realm><args><arg name='unused' type='number'/><arg name='another' type='number'/></args><rets><ret type='boolean'>Return true to stop drawing.</ret></rets></function>"),
        "Global.OrdinaryReturnProbe": page("Global.OrdinaryReturnProbe", "<function name='OrdinaryReturnProbe'><file line='22'>lua/installed.lua</file><rets><ret type='boolean'/></rets></function>"),
        "util.AnyNativeAlias": page("util.AnyNativeAlias", "<function name='AnyNativeAlias' parent='util' type='libraryfunc'><file line='23'>lua/installed.lua</file></function>"),
        "util.AnotherNativeAlias": page("util.AnotherNativeAlias", "<function name='AnotherNativeAlias' parent='util' type='libraryfunc'><file line='25'>lua/installed.lua</file></function>"),
        "Entity:NetworkVar": page("Entity:NetworkVar", "<function name='NetworkVar' parent='Entity' type='classfunc'><file line='3'>lua/installed.lua</file><args><arg name='typename'/><arg name='index'/><arg name='name'/><arg name='other_data'/></args></function>"),
        "Entity:NoReceiver": page("Entity:NoReceiver", "<function name='NoReceiver' parent='Entity' type='classfunc'><file line='5'>lua/installed.lua</file></function>"),
        "Entity:ShadowedCallback": page("Entity:ShadowedCallback", "<function name='ShadowedCallback' parent='Entity'><file line='7'>lua/installed.lua</file></function>"),
        "Entity:ParameterCallback": page("Entity:ParameterCallback", "<function name='ParameterCallback' parent='Entity'><file line='10'>lua/installed.lua</file></function>"),
        "Entity:AmbiguousInstalled": page("Entity:AmbiguousInstalled", "<function name='AmbiguousInstalled' parent='Entity'><file line='12'>lua/installed.lua</file></function>"),
        "Entity:UnknownCallback": page("Entity:UnknownCallback", "<function name='UnknownCallback' parent='Entity'><file line='14'>lua/installed.lua</file></function>"),
        "Entity:ForeignInstalled": page("Entity:ForeignInstalled", "<function name='NetworkVar' parent='Entity'><file line='1'>lua/probe.lua</file></function>"),
        "PLAYER:Spawn": page("PLAYER:Spawn", "<function name='Spawn' parent='PLAYER' type='hook'><file line='2'>gamemodes/base/gamemode/player_class/player_default.lua</file></function>"),
        "Player:Spawn": page("Player:Spawn", "<function name='Spawn' parent='Player'><file line='2'>gamemodes/base/gamemode/player_class/player_default.lua</file></function>"),
        "NEXTBOT:BehaveStart": page("NEXTBOT:BehaveStart", "<function name='BehaveStart' parent='NEXTBOT' type='hook'><file line='1'>gamemodes/base/entities/entities/base_nextbot/sv_nextbot.lua</file></function>"),
        "NextBot:BehaveStart": page("NextBot:BehaveStart", "<function name='BehaveStart' parent='NextBot' type='hook'><file line='1'>gamemodes/base/entities/entities/base_nextbot/sv_nextbot.lua</file></function>"),
        "NextBot:Foreign": page("NextBot:Foreign", "<function name='DoingEngineSchedule' parent='NextBot' type='hook'><file line='1'>gamemodes/base/entities/entities/base_ai/schedules.lua</file></function>"),
        "Task:Start": page("Task:Start", "<function name='Start' parent='Task' type='classfunc'><file line='5'>lua/includes/modules/ai_task.lua</file><args><arg name='npc'/></args></function>"),
        "Schedule:Init": page("Schedule:Init", "<function name='Init' parent='Schedule' type='classfunc'><file line='5'>lua/includes/modules/ai_schedule.lua</file></function>"),
        "ENTITY:DoingEngineSchedule": page("ENTITY:DoingEngineSchedule", "<function name='DoingEngineSchedule' parent='ENTITY' type='hook'><realm>Server</realm><file line='1'>gamemodes/base/entities/entities/base_ai/schedules.lua</file><rets><ret type='boolean'/></rets></function>"),
        "ENTITY:Ambiguous": page("ENTITY:Ambiguous", "<function name='Ambiguous' parent='ENTITY' type='hook'><file line='1'>gamemodes/base/entities/entities/base_ai/ambiguous.lua</file></function>"),
        "ENTITY:Foreign": page("ENTITY:Foreign", "<function name='DoingEngineSchedule' parent='ENTITY' type='hook'><file line='1'>lua/probe.lua</file></function>"),
        "WEAPON:WeaponProbe": page("WEAPON:WeaponProbe", "<function name='WeaponProbe' parent='WEAPON' type='hook'><file line='1'>gamemodes/base/entities/weapons/test/shared.lua</file></function>"),
        "Global.MenuGuardOnly": page("Global.MenuGuardOnly", "<function name='MenuGuardOnly'><realm>Shared</realm><file line='1'>lua/policy.lua</file></function>"),
        "Global.LoadedMenuOnly": page("Global.LoadedMenuOnly", "<function name='LoadedMenuOnly'><realm>Shared</realm><file line='1'>lua/outside_menu.lua</file></function>"),
        "Global.NativeExampleProbe": page("Global.NativeExampleProbe", "<function name='NativeExampleProbe'><realm>Shared</realm></function>"),
        "Global.ExamplePresent": page("Global.ExamplePresent", "<function name='ExamplePresent'><realm>Shared</realm></function><example><code>print('ok')</code></example>"),
        "Global.FakeExample": page("Global.FakeExample", "<function name='FakeExample'><realm>Shared</realm></function><code><example>not markup</example></code>"),
        "FixtureClass:__tostring": page("FixtureClass:__tostring", "<function name='__tostring' parent='FixtureClass' type='classfunc'><description><page>ConfirmedDeadTarget</page></description></function>"),
    })

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            address = unquote(urlparse(self.path).path.removeprefix("/gmod/"))
            state["requests"].append(address)
            if address == "~pagelist":
                status = state["index_status"]
                body = [{"address": key, "updateCount": state["version"], "viewCount": 98765 if key == "Global.NativeExampleProbe" else 12} for key in state["pages"]]
            else:
                body = state["pages"].get(address, 404)
                status = body if isinstance(body, int) else 200
            payload = json.dumps(body).encode()
            record("http", address=address, status=status)
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

        def log_message(self, *args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    url = f"http://127.0.0.1:{server.server_port}"

    def cli(name, *arguments, source_repo=repo):
        return run(name, [str(binary), "--repo-path", str(source_repo), "--wiki-url", url,
                          "--cache-dir", str(root / "cache"), *arguments])

    try:
        feedback_pages = ["ENTITY:DoingEngineSchedule", "ENTITY:Ambiguous", "ENTITY:Foreign", "WEAPON:WeaponProbe", "Global.MenuGuardOnly", "Global.LoadedMenuOnly", "Global.NativeExampleProbe", "Global.ExamplePresent", "Global.FakeExample", "FixtureClass:__tostring"]
        (root / "feedback-pages.txt").write_text("\n".join(feedback_pages), encoding="utf-8")
        feedback = cli("feedback entry-point flow", "check-list", "feedback-pages.txt", "--output-report", "feedback.html")
        feedback_html = (root / "feedback.html").read_text(encoding="utf-8")
        from html.parser import HTMLParser
        class FeedbackRows(HTMLParser):
            def __init__(self): super().__init__(); self.current = None; self.rows = []; self.pages = []; self.address = None
            def handle_starttag(self, tag, attrs):
                if tag == "tr" and dict(attrs).get("data-category"): self.current = [dict(attrs)["data-category"], ""]; self.address = None
                if tag == "a" and self.current is not None and self.address is None and "wiki.facepunch.com/gmod/" in dict(attrs).get("href", ""):
                    self.address = unquote(dict(attrs)["href"].split("/gmod/", 1)[1])
            def handle_data(self, data):
                if self.current is not None: self.current[1] += data
            def handle_endtag(self, tag):
                if tag == "tr" and self.current is not None: self.rows.append(self.current); self.pages.append((self.current[0], self.address)); self.current = None
        rows = FeedbackRows(); rows.feed(feedback_html)
        installed_pages = ["Entity:NetworkVar", "Entity:NoReceiver", "Entity:ShadowedCallback", "Entity:ParameterCallback", "Entity:AmbiguousInstalled", "Entity:UnknownCallback", "Entity:ForeignInstalled", "PLAYER:Spawn", "Player:Spawn", "NEXTBOT:BehaveStart", "NextBot:BehaveStart", "NextBot:Foreign", "Task:Start", "Schedule:Init", "Player:GetName", "Player:OtherAlias", "Player:ChainedAlias", "Player:ReboundAlias", "GM:HookProbe", "Global.OrdinaryReturnProbe", "util.AnyNativeAlias", "util.AnotherNativeAlias"]
        (root / "installed-pages.txt").write_text("\n".join(installed_pages), encoding="utf-8")
        installed = cli("installed and registered method ownership", "check-list", "installed-pages.txt", "--output-report", "installed.html")
        installed_rows = FeedbackRows(); installed_rows.feed((root / "installed.html").read_text(encoding="utf-8"))
        check("installed methods and registered hook source tags resolve", installed.returncode == 0 and all(not any(key == "unresolved" and name == address for key, name in installed_rows.pages) for address in ["Entity:NetworkVar", "Entity:NoReceiver", "PLAYER:Spawn", "NEXTBOT:BehaveStart", "NextBot:BehaveStart", "Task:Start"]))
        check("explicit ent receiver does not create a signature discrepancy", not any(key == "signature" and name == "Entity:NetworkVar" for key, name in installed_rows.pages))
        check("shadowed, ambiguous, unknown and foreign owners stay unresolved", all(any(key == "unresolved" and name == address for key, name in installed_rows.pages) for address in ["Entity:ShadowedCallback", "Entity:ParameterCallback", "Entity:AmbiguousInstalled", "Entity:UnknownCallback", "Entity:ForeignInstalled", "Player:Spawn", "NextBot:Foreign", "Schedule:Init"]))
        check("AST member aliases and alias chains resolve without special names", all(not any(key == "unresolved" and name == address for key, name in installed_rows.pages) for address in ["Player:GetName", "Player:OtherAlias", "Player:ChainedAlias", "util.AnyNativeAlias", "util.AnotherNativeAlias"]))
        check("native aliases do not invent contracts; rebound receiver stays unresolved", not any(key in ["signature", "returns"] and name in ["Player:GetName", "Player:OtherAlias", "Player:ChainedAlias"] for key, name in installed_rows.pages) and any(key == "unresolved" and name == "Player:ReboundAlias" for key, name in installed_rows.pages))
        check("hook defaults do not define callback contracts; ordinary returns still checked", not any(key in ["signature", "returns"] and name == "GM:HookProbe" for key, name in installed_rows.pages) and any(key == "returns" and name == "Global.OrdinaryReturnProbe" for key, name in installed_rows.pages))
        check("short sufficient docs retain a perfect quality score", any(key == "documentation-quality" and "Player:GetName" in text and "100/100" in text for key, text in installed_rows.rows))
        check("page views come from current official-index contract", '"Global.NativeExampleProbe":"98765"' in feedback_html and 'Page views' in feedback_html)
        check("tagged entity resolves without unrelated-owner signature", feedback.returncode == 0 and not any(name == "ENTITY:DoingEngineSchedule" and key in ["unresolved", "signature", "returns", "wrong-files", "line-ranges"] for key, name in rows.pages))
        check("ambiguous and foreign entity owners remain diagnostics", all(any(key == "unresolved" and name in text for key, text in rows.rows) for name in ["ENTITY:Ambiguous", "ENTITY:Foreign"]))
        check("tagged weapon resolves", not any("WEAPON:WeaponProbe" in text and key == "unresolved" for key, text in rows.rows))
        check("guarded and positively loaded menu definitions excluded", not any("MenuGuardOnly" in text or "LoadedMenuOnly" in text for key, text in rows.rows))
        check("native and fake-code example gaps are opportunities", all(any(key == "examples" and name in text for key, text in rows.rows) for name in ["Global.NativeExampleProbe", "Global.FakeExample"]))
        check("existing examples and hooks are not opportunities", not any(key == "examples" and any(name in text for name in ["Global.ExamplePresent", "ENTITY:DoingEngineSchedule", "FixtureClass:__tostring"]) for key, text in rows.rows))
        check("existing metamethod page retains dead link checks", any(key == "broken-link" and "FixtureClass:__tostring" in text for key, text in rows.rows))
        scan = cli("feedback discovery policy", "scan", "--output-report", "policy.html")
        policy = (root / "policy.html").read_text(encoding="utf-8")
        parsed_policy = FeedbackRows(); parsed_policy.feed(policy)
        missing_text = " ".join(text for key, text in parsed_policy.rows if key == "missing")
        check("instance callbacks, player hooks and factory methods are not missing-page suggestions", all(name not in missing_text for name in ["UndocumentedInstanceCallback", "UndocumentedHook", "UndocumentedFactoryMethod", "ShadowedCallback", "ParameterCallback", "UnknownCallback", "AmbiguousInstalled"]))
        check("menu-only and actual metamethod pages not discovered", all(name not in missing_text for name in ["MenuPathOnly", "MenuGuardOnly", "LoadedMenuOnly", "FixtureClass:__tostring", "FixtureClass:__add"]))
        check("client-menu and ordinary double-underscore exports retained", all(name in missing_text for name in ["SharedMenuProbe", "Global.__call", "FixtureLibrary.__helper", "FixtureClass:Ordinary"]))
        (root / "meta-pages.txt").write_text("Color:Copy\n", encoding="utf-8")
        meta_result = cli("registered metatable source lookup", "check-list", "meta-pages.txt", "--output-report", "meta.html")
        meta_report = (root / "meta.html").read_text(encoding="utf-8")
        check("registered metatable resolves exact source and line range", meta_result.returncode == 0 and
              "Color:Copy" in meta_report and "registered_meta.lua#L3" in meta_report and
              'data-category="line-ranges"' in meta_report and 'data-category="unresolved"' not in meta_report)
        scan = cli("registered metatable discovery", "scan", "--limit", "0", "--output-report", "meta-scan.html")
        # Discovery count is not used as proof of lookup; the report above exercises the real page path.
        check("metatable registration remains usable in discovery", scan.returncode == 0)
        result = cli("control overview discovery", "audit", "--prefix", "ControlProbe", "--output-report", "controls.html")
        controls = (root / "controls.html").read_text(encoding="utf-8")
        from html.parser import HTMLParser
        class Rows(HTMLParser):
            def __init__(self): super().__init__(); self.category = None; self.rows = []
            def handle_starttag(self, tag, attrs):
                if tag == "tr": self.category = dict(attrs).get("data-category")
                if tag == "a" and self.category: self.rows.append((self.category, dict(attrs).get("href", "")))
            def handle_endtag(self, tag):
                if tag == "tr": self.category = None
        parsed = Rows(); parsed.feed(controls)
        missing = [href for category, href in parsed.rows if category == "missing-controls" and "wiki.facepunch" in href]
        check("empty and inherited-only controls require their own overview", len(missing) == 2 and
              any(href.endswith("/ControlProbeEmpty") for href in missing) and
              any(href.endswith("/ControlProbeChild") for href in missing))
        check("inherited hook and shadowed registrations are not missing", not any(
              "OnStartDragging" in href or "ControlProbePrivate" in href for category, href in parsed.rows))
        check("failed control lookup is not reported missing", result.returncode != 0 and
              not any(href.endswith("/ControlProbeFailure") for href in missing) and
              any(category == "failed" and href.endswith("/ControlProbeFailure") for category, href in parsed.rows))
        check("missing controls carry commit-pinned registration evidence", "VGUI control registration: ControlProbeEmpty" in controls and
              f"/blob/{revision}/" in controls and "controls.lua#L" in controls)
        index_requests_before_cache_flow = state["requests"].count("~pagelist")
        (root / "pages.txt").write_text("Global.Probe\n", encoding="utf-8")
        result = cli("clean snapshot", "check-list", "pages.txt", "--output-report", "clean.html")
        clean = (root / "clean.html").read_text(encoding="utf-8") if (root / "clean.html").exists() else ""
        check("findings are successful and evidence pins committed line", result.returncode == 0 and
              f"/blob/{revision}/garrysmod/lua/probe.lua#L1" in clean)
        (root / "realm.txt").write_text("Global.RealmProbe\n", encoding="utf-8")
        result = cli("realm evidence identity", "check-list", "realm.txt", "--output-report", "realm.html")
        realm_report = (root / "realm.html").read_text(encoding="utf-8") if (root / "realm.html").exists() else ""
        check("realm evidence keeps the selected definition's file", result.returncode == 0 and
              f"/blob/{revision}/garrysmod/lua/server/realm.lua#L1" in realm_report and
              f"/blob/{revision}/garrysmod/lua/client/realm.lua#L1" not in realm_report)
        source.write_text("\n\n\n\nfunction Probe() end\n", encoding="utf-8")
        def audit_record(html):
            import re
            return re.sub(r'<time id="audit-completed"[^>]*>.*?</time>', '', html)
        (lua / "untracked.lua").write_bytes(b"\xff")
        result = cli("dirty snapshot", "check-list", "pages.txt", "--output-report", "dirty.html")
        dirty = (root / "dirty.html").read_text(encoding="utf-8") if (root / "dirty.html").exists() else ""
        check("dirty and untracked files cannot change committed evidence", result.returncode == 0 and audit_record(clean) == audit_record(dirty))
        check("valid cache reused after fresh index", state["requests"].count("Global.Probe") == 1 and
              state["requests"].count("~pagelist") == index_requests_before_cache_flow + 3)
        state["version"] = 2
        state["pages"]["Global.Probe"]["markup"] = "<function name='Probe'><file line='1'>lua/probe.lua</file></function>"
        result = cli("stale cache", "check-list", "pages.txt", "--output-report", "stale.html")
        stale_rows = FeedbackRows(); stale_rows.feed((root / "stale.html").read_text(encoding="utf-8"))
        check("changed index refetches stale page", result.returncode == 0 and
              not any(key in ["line-ranges", "unresolved", "failed"] for key, text in stale_rows.rows) and state["requests"].count("Global.Probe") == 2)
        for address, title in [("Review:A", "Colon page"), ("Review_A", "Underscore page"),
                               ("Review:A", "Colon page"), ("Review_A", "Underscore page")]:
            result = cli("cache collision", "check", address)
            check(f"correct cache identity {address}", result.returncode == 0 and title in result.stdout)
        check("colliding legacy names each downloaded once", state["requests"].count("Review:A") == 1 and
              state["requests"].count("Review_A") == 1)
        case_requests_before = state["requests"].count("Global.probe")
        (root / "case.txt").write_text("Global.probe\n", encoding="utf-8")
        result = cli("case-sensitive page identity", "check-list", "case.txt", "--output-report", "case.html")
        case_rows = FeedbackRows(); case_rows.feed((root / "case.html").read_text(encoding="utf-8"))
        check("Windows cache preserves case-sensitive identity", result.returncode == 0 and
              not any(key in ["line-ranges", "unresolved", "failed"] for key, text in case_rows.rows) and state["requests"].count("Global.probe") == case_requests_before + 1)
        second = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        second_thread = threading.Thread(target=second.serve_forever, daemon=True)
        second_thread.start()
        original_url = url
        try:
            url = f"http://127.0.0.1:{second.server_port}"
            state["pages"]["Review:A"]["title"] = "Other origin page"
            result = cli("cache origin isolation", "check", "Review:A")
            check("different origins cannot share cached pages", result.returncode == 0 and
                  "Other origin page" in result.stdout and state["requests"].count("Review:A") == 2)
        finally:
            url = original_url
            state["pages"]["Review:A"]["title"] = "Colon page"
            second.shutdown()
            second.server_close()
            second_thread.join()
        for path in (root / "cache").glob("v2-*.json"):
            entry = json.loads(path.read_text(encoding="utf-8"))
            if entry["requested_address"] == "Global.Probe":
                entry["requested_address"] = "DifferentPage"
                path.write_text(json.dumps(entry), encoding="utf-8")
        before = state["requests"].count("Global.Probe")
        result = cli("mismatched cache payload", "check", "Global.Probe")
        check("payload identity mismatch forces refresh", result.returncode == 0 and
              state["requests"].count("Global.Probe") == before + 1)
        state["index_status"] = 503
        result = cli("index failure", "check", "Global.Probe")
        check("index failure does not silently reuse cache", result.returncode != 0 and "503" in result.stderr)
        state["index_status"] = 200
        result = cli("single page HTTP failure", "check", "Failure")
        check("failed single check exits nonzero", result.returncode != 0 and "500" in result.stderr)
        (root / "pages.txt").write_text("Global.Probe\nFailure\n", encoding="utf-8")
        result = cli("partial batch", "check-list", "pages.txt", "--output-report", "partial.html")
        partial = (root / "partial.html").read_text(encoding="utf-8") if (root / "partial.html").exists() else ""
        check("partial batch retains report and exits nonzero", result.returncode != 0 and
              "Failure" in partial and "500" in partial and "Audit incomplete" in result.stderr)
        (root / "outside.lua").write_text("function Outside() end", encoding="utf-8")
        result = cli("source path traversal", "check", "Traversal")
        check("source tags cannot inspect outside files", result.returncode == 0 and
              "FILE NOT FOUND" in result.stdout and "LINE RANGE DRIFT" not in result.stdout)
        (root / "oversized.txt").write_text("Oversized\n", encoding="utf-8")
        result = cli("oversized range", "check-list", "oversized.txt", "--output-report", "oversized.html")
        check("untrusted line numbers cannot overflow severity scoring", result.returncode == 0 and
              (root / "oversized.html").exists())
        source.write_bytes(b"\xff\xfe\xff")
        git("add", "garrysmod/lua/probe.lua")
        git("commit", "-qm", "invalid encoding")
        result = cli("invalid committed encoding", "scan", "--limit", "0", "--output-report", "invalid.html")
        check("unreadable committed source fails coverage", result.returncode != 0 and
              "Cannot index committed source" in result.stderr and not (root / "invalid.html").exists())
        source.write_text("function Probe(\n", encoding="utf-8")
        git("add", "garrysmod/lua/probe.lua")
        git("commit", "-qm", "invalid syntax")
        result = cli("invalid committed syntax", "scan", "--limit", "0", "--output-report", "syntax.html")
        check("parser errors fail coverage", result.returncode != 0 and "Incomplete source analysis" in result.stderr)
        source.write_text(original, encoding="utf-8")
        git("add", "garrysmod/lua/probe.lua")
        git("commit", "-qm", "restore source")
        git("remote", "set-url", "origin", str(root / "absent-remote"))
        result = cli("explicit pull failure", "--pull", "scan", "--limit", "0")
        check("failed requested pull stops audit", result.returncode != 0 and "git pull failed" in result.stderr)
        if args.repo:
            before = len(state["requests"])
            result = cli("real repository source coverage", "scan", "--limit", "0",
                         "--output-report", "real-source.html", source_repo=args.repo.resolve())
            check("real committed repository indexes without HTTP", result.returncode == 0 and
                  len(state["requests"]) == before)
        result = run("HTML summary tool", ["python", str(project / "scripts/analyze_report.py"), "clean.html"])
        check("summary reads current HTML", result.returncode == 0 and "Global.Probe" in result.stdout)
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
        record("summary", failures=failures)
        log.close()
        print(json.dumps({"log": str(log_path), "failures": failures}))
    return bool(failures)


if __name__ == "__main__":
    raise SystemExit(main())
