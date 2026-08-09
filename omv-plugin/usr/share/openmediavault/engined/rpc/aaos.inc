<?php

/**
 * openmediavault-aaos plugin - AAOS RPC service.
 *
 * 把 OMV 的 JSON-RPC 桥接到 AAOS Core 的 Unix socket。
 * 调用形式:omv-rpc "AAOS" "query" '{"input":"列出磁盘"}'
 *            omv-rpc "AAOS" "query" '{"confirmation_token":"..."}'
 *
 * @license   https://www.gnu.org/licenses/gpl.html GPL Version 3
 */

namespace Engined\Rpc;

require_once("openmediavault/functions.inc");

class OMVRpcServiceAAOS extends \OMV\Rpc\ServiceAbstract
{
    /// AAOS Core 的 IPC socket
    const CORE_SOCKET = "unix:///run/aaos/core.sock";
    /// 读写超时(秒)-- applyChanges 等可能较久
    const SOCKET_TIMEOUT = 60;

    public function getName()
    {
        return "AAOS";
    }

    public function initialize()
    {
        $this->registerMethod("query");
        $this->registerMethod("status");
        $this->registerMethod("getConfig");
        $this->registerMethod("setConfig");
    }

    /**
     * 向 AAOS Core 发一条自然语言请求(或带 confirmation_token 确认破坏性操作)。
     * 返回:{status: success|error|needs_confirmation, output, confirmation_token}
     */
    public function query($params, $context)
    {
        $this->validateMethodContext($context, [
            "role" => OMV_ROLE_ADMINISTRATOR
        ]);
        $input = strval($params['input'] ?? '');
        $confirm = $params['confirmation_token'] ?? null;
        $safePwd = $params['safe_pwd'] ?? null;
        if ($input === '' && $confirm === null) {
            throw new \OMV\BaseException(
                "Either 'input' or 'confirmation_token' is required."
            );
        }

        // 构造 IPC Request 消息(JSON line)
        $req = [
            "type" => "request",
            "id" => bin2hex(random_bytes(8)),
            "input" => $input,
            "confirmation_token" => $confirm,
            "safe_pwd" => $safePwd,
        ];
        $line = json_encode($req) . "\n";

        $resp = $this->callCore($line);
        if (!is_array($resp)) {
            throw new \OMV\BaseException("Invalid response from aaos-core.");
        }
        return $resp;
    }

    /**
     * 报告 AAOS Core/Sentinel 进程状态。
     */
    public function status($params, $context)
    {
        $this->validateMethodContext($context, [
            "role" => OMV_ROLE_ADMINISTRATOR
        ]);
        $coreSock = file_exists("/run/aaos/core.sock");
        $sentinelSock = file_exists("/run/aaos/sentinel.sock");
        return [
            "core_running" => $coreSock,
            "sentinel_running" => $sentinelSock,
            "rules_loaded" => file_exists("/etc/aaos/rules.json"),
        ];
    }

    /**
     * 连接 aaos-core socket,发一行请求,读一行响应。
     */
    private function callCore($line)
    {
        $sock = @stream_socket_client(self::CORE_SOCKET, $errno, $errstr, 5);
        if ($sock === false) {
            throw new \OMV\BaseException(sprintf(
                "Cannot connect to aaos-core (%s): %s. Is aaos-core.service running?",
                self::CORE_SOCKET, $errstr
            ));
        }
        stream_set_timeout($sock, self::SOCKET_TIMEOUT);
        fwrite($sock, $line);

        // 读到第一个换行(一条 JSON 消息)
        $respLine = "";
        while (!feof($sock)) {
            $chunk = fgets($sock, 4096);
            if ($chunk === false) {
                break;
            }
            $respLine .= $chunk;
            if (strpos($respLine, "\n") !== false) {
                break;
            }
        }
        fclose($sock);

        return json_decode(trim($respLine), true);
    }

    /**
     * 获取 AAOS 配置(env + models.toml),key 脱敏。
     */
    public function getConfig($params, $context)
    {
        $this->validateMethodContext($context, ["role" => OMV_ROLE_ADMINISTRATOR]);
        $envPath = "/etc/aaos/env";
        $modelsPath = "/etc/aaos/models.toml";

        // 读 env(key 脱敏:只显示尾4位)
        $env = [];
        if (file_exists($envPath)) {
            foreach (file($envPath, FILE_IGNORE_NEW_LINES) as $line) {
                if (strpos($line, '=') === false) continue;
                [$k, $v] = explode('=', $line, 2);
                if (strlen($v) > 4) {
                    $env[$k] = str_repeat('*', strlen($v) - 4) . substr($v, -4);
                } else {
                    $env[$k] = '****';
                }
            }
        }

        // 读 models.toml(原样返回,不含 key)
        $models = file_exists($modelsPath) ? file_get_contents($modelsPath) : "";

        return [
            "env" => $env,
            "models_toml" => $models,
        ];
    }

    /**
     * 更新 AAOS 配置(env + models.toml),重启 core。
     * params: {env: {KEY: value}, models_toml: "..."}
     */
    public function setConfig($params, $context)
    {
        $this->validateMethodContext($context, ["role" => OMV_ROLE_ADMINISTRATOR]);
        $envPath = "/etc/aaos/env";
        $modelsPath = "/etc/aaos/models.toml";

        // 更新 env(只更新传入的 key,保留其他)
        if (isset($params['env']) && is_array($params['env'])) {
            $existing = [];
            if (file_exists($envPath)) {
                foreach (file($envPath, FILE_IGNORE_NEW_LINES) as $line) {
                    if (strpos($line, '=') === false) continue;
                    [$k, $v] = explode('=', $line, 2);
                    $existing[$k] = $v;
                }
            }
            foreach ($params['env'] as $k => $v) {
                // 跳过脱敏值(尾4位 ***xxxx),只写真实值
                if (strpos($v, '***') === 0 && strlen($v) <= 8) continue;
                $existing[$k] = $v;
            }
            $content = "";
            foreach ($existing as $k => $v) {
                $content .= "$k=$v\n";
            }
            file_put_contents($envPath, $content);
            chmod($envPath, 0600);
        }

        // 更新 models.toml
        if (isset($params['models_toml']) && !empty($params['models_toml'])) {
            file_put_contents($modelsPath, $params['models_toml']);
        }

        // 重启 aaos-core
        exec("systemctl restart aaos-core 2>&1", $output, $rc);

        return [
            "success" => $rc === 0,
            "message" => $rc === 0 ? "配置已更新,aaos-core 已重启" : "重启失败: " . implode("\n", $output),
        ];
    }
}
