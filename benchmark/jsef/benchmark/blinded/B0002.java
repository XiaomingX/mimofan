/*
 * JSEF Benchmark 样本 — 认证保护修复：后端在每次敏感操作前真实重校验会话/凭据（CWE-522，L4）
 * 独立 benchmark 源文件，不强求编译。安全底线：仅 localhost 演示语义，不写真实利用脚本。
 *
 * 修复要点：不信任客户端提交的"已认证"标记，操作前从 HttpSession 读取服务端
 * 认证态并重校验，缺失会话则拒绝进入敏感操作。
 */
package blinded;

import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpSession;

public class AuthzBypassClientOnlyBy {

    



    void adminOperation(String operator) {
        System.out.println("[admin-op] operator=" + operator); // 语义等价：执行管理员敏感操作
    }

    


    public String openAdmin(HttpServletRequest request) {
        HttpSession session = request.getSession(false); // 不创建新会话
        /*ANCHOR_1*/
        if (session == null || session.getAttribute("authenticatedUser") == null) {
            return "forbidden"; // 后端拒绝：无服务端认证态
        }
        adminOperation((String) session.getAttribute("authenticatedUser")); // 身份来自服务端会话
        return "processed";
    }
}
