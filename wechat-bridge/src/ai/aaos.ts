import net from 'node:net';

/**
 * AAOS 微信桥:把微信消息转发到 AAOS core.sock,把回复发回微信。
 * AAOS Core 无状态(每次 Request 独立处理),不维护多轮对话历史。
 */
export class AaosChat {
  private socketPath: string;
  private allowedUsers: Set<string>;

  constructor(opts: { socketPath?: string; allowedUsers?: string[] }) {
    this.socketPath = opts.socketPath || '/run/aaos/core.sock';
    this.allowedUsers = new Set(opts.allowedUsers || []);
  }

  /** AAOS Core 无状态,无需清理 session */
  clearSession(_userId: string): void {}

  async chat(userId: string, userMessage: string): Promise<string> {
    // 鉴权:白名单非空时,只允许白名单用户控制 NAS
    if (this.allowedUsers.size > 0 && !this.allowedUsers.has(userId)) {
      return '未授权:你不在此 NAS 的允许列表中。';
    }

    return new Promise((resolve, reject) => {
      const sock = net.createConnection(this.socketPath);
      const id = Date.now().toString() + Math.random().toString(36).slice(2, 8);
      const req =
        JSON.stringify({
          type: 'request',
          id,
          input: userMessage,
          confirmation_token: null,
        }) + '\n';

      let buf = '';
      sock.setTimeout(60_000); // AAOS LLM 调度可能慢

      sock.on('connect', () => sock.write(req));
      sock.on('data', (data) => {
        buf += data.toString();
        if (buf.includes('\n')) {
          sock.end();
          try {
            const resp = JSON.parse(buf.trim());
            if (resp.status === 'needs_confirmation') {
              resolve(`⚠️ 破坏性操作,需确认:\n${resp.output}\n\n(微信暂不支持确认,请到 Web UI 操作)`);
            } else if (resp.status === 'error') {
              resolve(`❌ ${resp.output}`);
            } else {
              resolve(resp.output || '(AAOS 无回复)');
            }
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
