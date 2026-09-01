package blinded;

import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

import javax.websocket.OnClose;
import javax.websocket.OnMessage;
import javax.websocket.OnOpen;
import javax.websocket.ServerEndpoint;
import javax.websocket.Session;


















@ServerEndpoint("/chat")
public class WebSocketNoAuthzBx {

    
    private final Map<String, String> sessionIdentity = new ConcurrentHashMap<>();

    
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
        
        /*ANCHOR_1*/
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
