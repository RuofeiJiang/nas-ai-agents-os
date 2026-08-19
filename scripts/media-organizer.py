#!/usr/bin/env python3
"""媒体归位器:qbit 下载完成钩子,纯规则分类,软链入媒体树,通知 Jellyfin。
用法: media-organizer.py "%N" "%F" "%L"   (名称/内容路径/分类)
分层决策: qbit分类 > 文件名规则 > 未分类(通知人工)。零 LLM。"""
import sys, os, re, json, time, pathlib, urllib.request

MEDIA = "/srv/dev-disk-by-uuid-b4d3836a-adfe-41fc-a4b6-1fe796076ae5/media"
JF_URL = "http://127.0.0.1:8096"
INBOX = pathlib.Path("/var/lib/aaos/inbox.jsonl")
LOG = "/var/log/media-organizer.log"

name, content, category = (sys.argv[1] if len(sys.argv)>1 else "",
                           sys.argv[2] if len(sys.argv)>2 else "",
                           sys.argv[3] if len(sys.argv)>3 else "")

def log(msg):
    line = time.strftime("%F %T") + " " + msg
    with open(LOG, "a") as f: f.write(line + "\n")

def notify(title, text):
    try:
        INBOX.parent.mkdir(parents=True, exist_ok=True)
        with open(INBOX, "a") as f:
            f.write(json.dumps({"id": int(time.time()*1000), "time": time.strftime("%F %T"),
                "title": title, "text": text}) + "\n")
    except Exception: pass

def classify(name, category):
    """规则优先级:显式分类 > 发布组方括号(动漫) > SxxExx/季模式(剧集) > 电影"""
    if category in ("anime","tv","movie"): return category
    if re.search(r"^\s*\[[^\]]+\]", name): return "anime"        # [VCB-Studio]... 发布组开头
    if re.search(r"[Ss]\d{1,2}[Ee]\d{1,2}|\bS\d{2}\b|[Ss]eason", name): return "tv"
    if re.search(r"[一-鿿]", name): return "movie"       # 含中文默认电影
    return None

def jellyfin_scan():
    try:
        urllib.request.urlopen(urllib.request.Request(
            JF_URL + "/Library/Refresh", method="POST"), timeout=8)
    except Exception: pass   # 未初始化/无key 时静默

if not os.path.exists(content):
    log("[跳过] 路径不存在: " + content); sys.exit(0)

cat = classify(name, category)
if cat is None:
    log("[未分类] " + name)
    notify("媒体归位:需要人工分类", name + chr(10) + "在 qbit 里设置分类(anime/tv/movie)后重新触发")
    sys.exit(0)

dest_dir = os.path.join(MEDIA, cat)
os.makedirs(dest_dir, exist_ok=True)
link = os.path.join(dest_dir, name)
if os.path.lexists(link):
    log("[已存在] " + link); sys.exit(0)
os.symlink(content, link)
log("[归位] %s -> %s" % (link, cat))
notify("媒体已归位", name + chr(10) + "分类: " + cat)
jellyfin_scan()
