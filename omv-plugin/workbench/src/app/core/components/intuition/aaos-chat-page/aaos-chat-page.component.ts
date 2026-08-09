import { Component, AfterViewInit, ElementRef } from '@angular/core';
import { RpcService } from '~/app/shared/services/rpc.service';

interface ChatMessage {
  role: 'user' | 'assistant';
  text: string;
  status?: string;
  confirmToken?: string | null;
  needsPwd?: boolean;
  html?: string;
}

// 简单 Markdown -> HTML(不引入额外库)
function mdToHtml(md: string): string {
  let html = md;
  // 代码块
  html = html.replace(/```(\w*)\n([\s\S]*?)```/g, '<pre><code>$2</code></pre>');
  // 表格(简单处理)
  const lines = html.split('\n');
  let inTable = false;
  let tableHtml = '';
  const out: string[] = [];
  for (const line of lines) {
    if (line.trim().startsWith('|') && line.trim().endsWith('|')) {
      const cells = line.split('|').filter((_, i, a) => i > 0 && i < a.length - 1).map(c => c.trim());
      if (cells.every(c => /^[-:]+$/.test(c))) continue; // 分隔行跳过
      if (!inTable) { inTable = true; tableHtml = '<table>'; }
      const tag = tableHtml.includes('<th>') ? 'td' : 'th';
      tableHtml += '<tr>' + cells.map(c => `<${tag}>${c}</${tag}>`).join('') + '</tr>';
      // 第一行后补 th
      if (tag === 'th') tableHtml = tableHtml.replace('<th>', '<thead><tr><th>').replace('</tr>', '</tr></thead><tbody>');
      continue;
    }
    if (inTable) { out.push(tableHtml + '</tbody></table>'); inTable = false; tableHtml = ''; }
    out.push(line);
  }
  if (inTable) out.push(tableHtml + '</tbody></table>');
  html = out.join('\n');
  // 标题
  html = html.replace(/^### (.+)$/gm, '<h4>$1</h4>');
  html = html.replace(/^## (.+)$/gm, '<h3>$1</h3>');
  html = html.replace(/^# (.+)$/gm, '<h2>$1</h2>');
  // 粗体
  html = html.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
  // 行内代码
  html = html.replace(/`([^`]+)`/g, '<code>$1</code>');
  // 换行
  html = html.replace(/\n/g, '<br>');
  return html;
}

@Component({
  selector: 'omv-intuition-aaos-chat-page',
  templateUrl: './aaos-chat-page.component.html',
  styleUrls: ['./aaos-chat-page.component.scss']
})
export class AaosChatPageComponent implements AfterViewInit {
  messages: ChatMessage[] = [];
  input = '';
  loading = false;

  showSettings = false;
  configLoading = false;
  configSaving = false;
  envKeys: {key: string, value: string, editing: boolean}[] = [];
  modelsToml = '';
  configMsg = '';

  // 快捷操作
  quickActions = ['系统信息', '列出磁盘', '列出共享', '列出容器', 'HA设备', '应用配置'];

  constructor(private rpcService: RpcService, private el: ElementRef) {
    // 加载历史(localStorage)
    try {
      const saved = localStorage.getItem('aaos-chat-history');
      if (saved) {
        this.messages = JSON.parse(saved);
      }
    } catch (e) { /* ignore */ }
  }

  ngAfterViewInit(): void {
    this.scrollToBottom();
  }

  private saveHistory(): void {
    try {
      // 只保留最近 50 条
      const toSave = this.messages.slice(-50);
      localStorage.setItem('aaos-chat-history', JSON.stringify(toSave));
    } catch (e) { /* ignore */ }
  }

  private scrollToBottom(): void {
    setTimeout(() => {
      const container = this.el.nativeElement.querySelector('.aaos-chat__messages');
      if (container) container.scrollTop = container.scrollHeight;
    }, 50);
  }

  clearHistory(): void {
    this.messages = [];
    localStorage.removeItem('aaos-chat-history');
  }

  send(text?: string): void {
    const msg = (text || this.input).trim();
    if (!msg || this.loading) return;
    this.messages.push({ role: 'user', text: msg });
    this.input = '';
    this.call({ input: msg });
  }

  quickSend(action: string): void {
    if (this.loading) return;
    this.messages.push({ role: 'user', text: action });
    this.call({ input: action });
  }

  confirm(token: string, needsPwd: boolean = false): void {
    if (this.loading) return;
    let safePwd: string | null = null;
    if (needsPwd) {
      safePwd = window.prompt('⚠️ 破坏性操作,请输入安全口令:');
      if (safePwd === null) return;  // 用户取消
    }
    this.messages.push({ role: 'user', text: '确认执行' });
    this.call({ confirmation_token: token, safe_pwd: safePwd });
  }

  private call(params: Record<string, unknown>): void {
    this.loading = true;
    this.scrollToBottom();
    this.rpcService.request('AAOS', 'query', params).subscribe({
      next: (res: any) => {
        this.loading = false;
        const r = res?.response ?? res ?? {};
        const text = r.output ?? '(无输出)';
        this.messages.push({
          role: 'assistant',
          text,
          html: mdToHtml(text),
          status: r.status,
          confirmToken: r.confirmation_token ?? null,
          needsPwd: typeof text === 'string' && text.includes('--safe-pwd')
        });
        this.saveHistory();
        this.scrollToBottom();
      },
      error: (err: any) => {
        this.loading = false;
        const text = '❌ 调用失败: ' + (err?.message ?? JSON.stringify(err));
        this.messages.push({ role: 'assistant', text, html: mdToHtml(text) });
        this.saveHistory();
        this.scrollToBottom();
      }
    });
  }

  // ===== 设置面板 =====
  toggleSettings(): void {
    this.showSettings = !this.showSettings;
    if (this.showSettings && this.envKeys.length === 0) this.loadConfig();
  }

  loadConfig(): void {
    this.configLoading = true;
    this.configMsg = '';
    this.rpcService.request('AAOS', 'getConfig').subscribe({
      next: (res: any) => {
        this.configLoading = false;
        const data = res?.response ?? res ?? {};
        const env = data.env ?? {};
        this.envKeys = Object.keys(env).map(key => ({ key, value: env[key], editing: false }));
        this.modelsToml = data.models_toml ?? '';
      },
      error: (err: any) => {
        this.configLoading = false;
        this.configMsg = '加载失败: ' + (err?.message ?? '');
      }
    });
  }

  editKey(idx: number): void {
    this.envKeys[idx].editing = true;
    this.envKeys[idx].value = '';
  }

  saveConfig(): void {
    this.configSaving = true;
    this.configMsg = '';
    const env: Record<string, string> = {};
    for (const item of this.envKeys) {
      if (item.editing && item.value) env[item.key] = item.value;
    }
    const params: Record<string, unknown> = {};
    if (Object.keys(env).length > 0) params.env = env;
    if (this.modelsToml) params.models_toml = this.modelsToml;
    this.rpcService.request('AAOS', 'setConfig', params).subscribe({
      next: (res: any) => {
        this.configSaving = false;
        const data = res?.response ?? res ?? {};
        this.configMsg = data.message ?? '已保存';
        this.envKeys.forEach(item => item.editing = false);
        this.loadConfig();
      },
      error: (err: any) => {
        this.configSaving = false;
        this.configMsg = '保存失败: ' + (err?.message ?? '');
      }
    });
  }
}
