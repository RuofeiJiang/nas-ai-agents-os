#!/usr/bin/env python3
"""PT 下载看门狗:任务创建 3 分钟后仍未产生下载,自动诊断原因并告警。
去重:同一种子同一原因只告警一次,原因变化才重新告警。"""
import json, time, urllib.request, urllib.parse, http.cookiejar, pathlib, sys

QBIT="http://127.0.0.1:8080"
USER, PASS = "admin", "adminadmin"
GRACE = 180          # 创建后宽限秒数
STATE = pathlib.Path("/var/lib/pt-watchdog.json")
LOG = pathlib.Path("/var/log/pt-watchdog.log")

cj = http.cookiejar.CookieJar()
op = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(cj))
d = urllib.parse.urlencode({"username": USER, "password": PASS}).encode()
op.open(urllib.request.Request(QBIT+"/api/v2/auth/login", data=d), timeout=8)

def trackers(h):
    return [x for x in json.loads(op.open(QBIT+f"/api/v2/torrents/trackers?hash={h}", timeout=8).read()) if x["url"].startswith("http")]

def diagnose(t):
    """返回原因字符串"""
    msgs = [x.get("msg","") for x in trackers(t["hash"])]
    m = " ".join(msgs).lower()
    if "blacklist" in m:
        return "端口被 PT 站封禁(检查 listen_port,6881 等常见端口已被拉黑)"
    if "only free" in m or "not authorized" in m or "permission" in m:
        return "账号权限不足(等级限制/只能下FREE种/账号被限)"
    if "passkey" in m:
        return "passkey 无效(种子与账号不匹配,重新下载种子文件)"
    seeds = [x.get("num_seeds",-1) for x in trackers(t["hash"])]
    if seeds and seeds[0] == 0:
        return "死种(站内 0 做种)"
    if t["state"] == "errored":
        return "种子错误状态: " + ";".join(msgs)[:80]
    if not any(msgs):
        return "tracker 无响应(检查网络/站点可达性)"
    return "tracker 正常但无速度(做种少或对端限速): " + ";".join(msgs)[:80]

alerted = json.loads(STATE.read_text()) if STATE.exists() else {}
healed = set(alerted.get("_healed", []))
now = time.time()
for t in json.loads(op.open(QBIT+"/api/v2/torrents/info", timeout=8).read()):
    if t["state"] in ("downloading","stalledUP","uploading","stoppedUP","pausedUP"):  continue
    if t["progress"] > 0.001 or t["dlspeed"] > 0:                    continue
    if now - t["added_on"] < GRACE:                                   continue
    reason = diagnose(t)
    if alerted.get(t["hash"]) == reason:    # 同因已告警,跳过
        continue
    if t["hash"] not in healed:             # 第一轮:先自愈(强制reannounce)
        healed[t["hash"]] = True
        try:
            op.open(urllib.request.Request(QBIT+"/api/v2/torrents/reannounce",
              data=urllib.parse.urlencode({"hashes": t["hash"]}).encode()), timeout=8)
        except Exception:
            pass
        continue
    alerted[t["hash"]] = reason
    line = time.strftime("%F %T") + " [卡种] " + t["name"][:50] + " | " + reason
    # 写 AAOS 收件箱(Web 聊天页轮询展示;以后微信桥也消费此文件)
    try:
        import pathlib as _pl
        _pl.Path("/var/lib/aaos").mkdir(parents=True, exist_ok=True)
        with open("/var/lib/aaos/inbox.jsonl", "a") as _f:
            _f.write(json.dumps({"id": int(time.time()*1000), "time": time.strftime("%F %T"),
                "title": "PT 下载卡种(自愈失败)", "text": t["name"][:50] + chr(10) + "原因: " + reason}) + chr(10))
    except Exception:
        pass
    print(line)
    with open(LOG, "a") as f: f.write(line + "\n")

alerted["_healed"] = sorted(healed)
STATE.write_text(json.dumps(alerted))
