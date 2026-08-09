import { login, clearCredentials } from "./weixin/auth.js";
import { AaosChat } from "./ai/aaos.js";
import { Bot } from "./bot.js";

async function main(): Promise<void> {
  if (process.argv.includes("--logout")) {
    clearCredentials();
    console.log("已清除登录凭证,下次启动需要重新扫码。");
    return;
  }

  const socketPath = process.env.AAOS_SOCKET || "/run/aaos/core.sock";
  const allowedUsers = (process.env.ALLOWED_USERS || "")
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);

  if (allowedUsers.length === 0) {
    console.warn("⚠️ ALLOWED_USERS 未配置,所有微信联系人都能控制 NAS(仅测试用,生产请配白名单)");
  } else {
    console.log(`✅ 白名单已配置: ${allowedUsers.length} 个用户`);
  }

  console.log(` connecting AAOS core.sock: ${socketPath}`);
  const credentials = await login();

  const ai = new AaosChat({ socketPath, allowedUsers });
  const bot = new Bot(credentials, ai);

  const shutdown = () => {
    console.log("\n正在关闭...");
    bot.stop();
    process.exit(0);
  };
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);

  await bot.start();
}

main().catch((err) => {
  console.error("启动失败:", err);
  process.exit(1);
});
