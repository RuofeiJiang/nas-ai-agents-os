import net from 'node:net';

interface AaosResponse {
  status: 'success' | 'error' | 'needs_confirmation';
  output: string;
  confirmation_token?: string | null;
}

/**
 * AAOS 微信桥:微信消息 -> AAOS core.sock -> 回复发回微信。
 * 破坏性操作:Core 返回 needs_confirmation(token),用户回复"确认 <安全口令>"执行。
 */
export class AaosChat {
  private socketPath: string;
  private allowedUsers: Set<string>;
  private pendingConfirmations = new Map<string, string>(); // userId -> token

  constructor(opts: { socketPath?: string; allowedUsers?: string[] }) {
    this.socketPath = opts.socketPath || '/run/aaos/core.sock';
    this.allowedUsers = new Set(opts.allowedUsers || []);
  }

  clearSession(userId: string): void {
    this.pendingConfirmations.delete(userId);
  }

  async chat(userId: string, userMessage: string): Promise<string> {
    if (this.allowedUsers.size > 0 && !this.allowedUsers.has(userId)) {
      return '未授权:你不在此 NAS 的允许列表中。';
    }

    // 破坏性确认:用户回复"确认 <口令>"
    const confirmMatch = userMessage.match(/^确认\s+(.+)$/);
    if (confirmMatch) {
      const pwd = confirmMatch[1].trim();
      const token = this.pendingConfirmations.get(userId);
      if (!token) {
        return '没有待确认的破坏性操作。';
      }
      this.pendingConfirmations.delete(userId);
      try {
        const resp = await this.request('', token, pwd);
        return resp.status === 'error' ? `❌ ${resp.output}` : (resp.output || '✅ 已执行');
      } catch (e) {
        return `❌ 确认失败: ${e}`;
      }
    }

    // 正常请求
    const resp = await this.request(userMessage, null, null);
    if (resp.status === 'needs_confirmation') {
      if (resp.confirmation_token) {
        this.pendingConfirmations.set(userId, resp.confirmation_token);
      }
      return `⚠️ 破坏性操作,需安全口令确认:\n${resp.output}\n\n回复"确认 <安全口令>"执行(如:确认 mypass)`;
    }
    return resp.status === 'error' ? `❌ ${resp.output}` : (resp.output || '(AAOS 无回复)');
  }

  private request(input: string, confirmToken: string | null, safePwd: string | null): Promise<AaosResponse> {
    return new Promise((resolve, reject) => {
      const sock = net.createConnection(this.socketPath);
      const id = Date.now().toString() + Math.random().toString(36).slice(2, 8);
      const req =
        JSON.stringify({
          type: 'request',
          id,
          input,
          confirmation_token: confirmToken,
          safe_pwd: safePwd,
        }) + '\n';

      let buf = '';
      sock.setTimeout(60_000);

      sock.on('connect', () => sock.write(req));
      sock.on('data', (data) => {
        buf += data.toString();
        if (buf.includes('\n')) {
          sock.end();
          try {
            resolve(JSON.parse(buf.trim()) as AaosResponse);
          } catch {
            reject(new Error('解析 AAOS 响应失败: ' + buf.slice(0, 200)));
          }
        }
      });
      sock.on('error', (err) => {
        reject(new Error(`连接 AAOS core.sock 失败 (${this.socketPath}): ${err.message}`));
      });
      sock.on('timeout', () => {
        sock.destroy();
        reject(new Error('AAOS 响应超时(>60s)'));
      });
    });
  }
}
