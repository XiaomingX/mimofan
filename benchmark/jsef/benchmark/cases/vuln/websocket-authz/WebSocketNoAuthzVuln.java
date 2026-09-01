package com.jsef.benchmark.vuln.websocketauthz;

import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

import javax.websocket.OnClose;
import javax.websocket.OnMessage;
import javax.websocket.OnOpen;
import javax.websocket.ServerEndpoint;
import javax.websocket.Session;

/**
 * JSEF-Benchmark L3 — WebSocket 缺鉴权（CWE-862）
 *
 * 语义：HTTP 握手阶段（升级 WebSocket 前）做一次鉴权，把用户身份存入
 * session.userProperties；但 @ServerEndpoint 的 onOpen/onMessage 阶段
 * 不再复核会话身份 —— 消息处理器直接信任消息内携带的 userId，用它操作
 * 敏感资源（转账）。
 *
 * 两段状态：握手（一次鉴权）→ 消息（无复核）。攻击者持有任意合法会话
 * （如低权限账号），即可在消息体内伪造他人 userId 越权操作。
 *
 * CWE-862 (Missing Authorization)。
 * 安全底线：仅 localhost 演示语义，不提供真实利用脚本。
 *
 * 修复要点（对照 WebSocketNoAuthzSafe.java）：onOpen 校验握手身份；
 * 消息处理每次用 session.getUserPrincipal() 复核，不信任消息内容。
 */
@ServerEndpoint("/chat")
public class WebSocketNoAuthzVuln {

    /** 会话 id -> 握手阶段写入的身份（之后不再复核） */
    private final Map<String, String> sessionIdentity = new ConcurrentHashMap<>();

    /** 节点1：HTTP 握手阶段唯一一次鉴权（可信来源：登录后的 token） */
    static String handshake(String token, Session session) {
        String userId = resolveUserIdByToken(token);
        session.getUserProperties().put("identity", userId); // 身份写入会话
        return userId;
    }

    @OnOpen
    public void onOpen(Session session) {
        // 节点2：onOpen 取出握手阶段写入的身份，存到服务端会话映射
        String identity = (String) session.getUserProperties().get("identity");
        sessionIdentity.put(session.getId(), identity);
    }

    @OnMessage
    public void onMessage(String raw, Session session) {
        // 消息内携带的 userId 完全不可信
        String claimedUserId = parseClaimedUserId(raw);
        int amount = parseAmount(raw);
        // [VULN] 漏洞点：未用服务端会话身份复核，直接信任消息内 userId
        // [CHECKPOINT id=JSEF-WS-001 cwe=862 level=L3 source=websocket message sink=message handler without session identity check expect=VULN trace=benchmark/cases/vuln/websocket-authz/WebSocketNoAuthzVuln.java:38,benchmark/cases/vuln/websocket-authz/WebSocketNoAuthzVuln.java:46,benchmark/cases/vuln/websocket-authz/WebSocketNoAuthzVuln.java:56]
        transfer(claimedUserId, amount, session);
    }

    @OnClose
    public void onClose(Session session) {
        sessionIdentity.remove(session.getId());
    }

    // ---- stubs（仅演示语义，不落真实实现）----

    static String resolveUserIdByToken(String token) {
        return "alice";
    }

    static String parseClaimedUserId(String raw) {
        // localhost 演示：从消息 JSON 提取 claimedUserId 字段
        return "alice";
    }

    static int parseAmount(String raw) {
        return 100;
    }

    static void transfer(String toUserId, int amount, Session session) {
        System.out.println("[ws-transfer] " + toUserId + " +" + amount);
    }
}
