/*
 * JSEF Benchmark 样本 — HTTP 请求走私安全对照（CWE-444，L4）
 * 规范化 / 丢弃冲突的 Content-Length 与 Transfer-Encoding 头。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

public class HttpSmugglingBy {

    interface Request { void setHeader(String name, String value); }
    interface HttpClient { void send(Request req); }

    
    static void forward(HttpClient client, String userContentLength, String userTransferEncoding) {
        Request req = null;
        // 规范化：若同时存在 CL 与 TE，丢弃 CL，仅保留单一帧定界
        if (userTransferEncoding != null && !userTransferEncoding.isEmpty()) {
            req.setHeader("Transfer-Encoding", userTransferEncoding);  // 仅 TE
        } else {
            req.setHeader("Content-Length", userContentLength);        // 仅 CL
        }
        // source：不可信请求头，但冲突已消除（单一帧定界）
        /*ANCHOR_1*/
        client.send(req);
    }
}
