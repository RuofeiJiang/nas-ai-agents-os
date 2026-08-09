import { AaosChat } from "./src/ai/aaos.js";

async function main(): Promise<void> {
  const socketPath = process.env.AAOS_SOCKET || "/run/aaos/core.sock";
  const ai = new AaosChat({ socketPath });
  console.log(`测试 AaosChat 连 core.sock (${socketPath})...`);

  const tests = ["列出磁盘", "系统信息"];
  for (const input of tests) {
    console.log(`\n>>> 发: ${input}`);
    try {
      const reply = await ai.chat("test-user", input);
      console.log(`<<< 回: ${reply.slice(0, 200)}`);
    } catch (e) {
      console.error(`!!! 失败: ${e}`);
    }
  }
}

main();
