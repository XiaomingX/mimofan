package blinded;

import org.springframework.web.bind.annotation.ModelAttribute;










public class ModelAttrDeepBindBy {

    public static class UserPrefs {
        private String role; // 即使被表单写入，也会被服务端覆写
        public String getRole() { return role; }
        public void setRole(String v) { this.role = v; }
    }

    public static class AccountForm {
        private String username;
        private UserPrefs prefs = new UserPrefs();
        public String getUsername() { return username; }
        public void setUsername(String v) { this.username = v; }
        public UserPrefs getPrefs() { return prefs; }
        public void setPrefs(UserPrefs v) { this.prefs = v; }
    }

    




    public String handleBy(@ModelAttribute AccountForm form) {
        // 服务端覆写：role 一律以白名单为准，丢弃表单中的污染值
        String role = resolveRoleFromTrustedSource(form.getUsername());
        /*ANCHOR_1*/
        if ("ADMIN".equals(role)) { // role 来自可信来源，非表单污染值
            return "granted-admin";
        }
        return "user";
    }

    // 可信角色解析：仅允许服务端判定，绝不采用表单传入的 role
    private String resolveRoleFromTrustedSource(String username) {
        return "USER"; // 固定白名单角色；真实系统中从可信会话/DB 取值
    }

    public static void main(String[] args) {
        AccountForm f = new AccountForm();
        f.getPrefs().setRole("ADMIN"); // 攻击者写入，但会被服务端覆写
        System.out.println(new ModelAttrDeepBindBy().handleBy(f));
    }
}
