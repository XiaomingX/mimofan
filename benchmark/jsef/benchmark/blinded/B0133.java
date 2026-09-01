/*
 * JSEF Benchmark 安全样本 — 批量赋值提权（A04，CWE-915，L3）
 * BX 版：使用仅含白名单字段的 DTO 绑定，丢弃 isAdmin 等特权字段。
 * 测试点：强 SAST/LLM 应识别 DTO 白名单设计而不报（TN）。
 * 运行态需 JSEF 依赖；独立 benchmark 源文件，不强求编译。
 */
public class MassAssignPrivEscBy {

    static class UserDTO { String username; }   // 白名单字段，无 isAdmin

    static class UserProfile { String username; boolean isAdmin; }

    


    static UserProfile bind(UserDTO dto) {
        UserProfile p = new UserProfile();
        p.username = dto.username;
        // 特权字段由服务端决定，不来自请求
        /*ANCHOR_1*/
        p.isAdmin = false;   // 默认普通用户，攻击者无法注入
        return p;
    }
}
