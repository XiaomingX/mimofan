package blinded;

/*
 * 运行态需 JSEF 依赖：本文件引用 Spring 语义（@ModelAttribute、DataBinder 深度绑定），
 * 用于静态分析 / LLM 阅读，不强求编译，但语义正确、可读。
 *
 * JSEF-Benchmark L4 — 深度绑定：@ModelAttribute 嵌套 POJO 属性路径覆盖
 *
 * 难度：L4（框架语义 / 深度绑定）。Spring 的 @ModelAttribute 会把整张 HTTP 表单
 * 按"属性路径"（如 form.role）深度绑定到嵌套对象。攻击者可通过
 * form.role=ADMIN 写入嵌套 POJO 的授权字段 role，随后 role 进入授权决策（sink）。
 *
 * 难点/区分点：污点不是显式赋值，而是框架在绑定阶段按属性路径隐式写入
 * 嵌套对象的危险字段（模型属性深度绑定 / mass-assignment）。
 *
 * CWE-915 (Mass Assignment / 业务权限提升)。
 *
 * 安全底线：仅展示绑定语义，Payload 仅 localhost 演示，不提供真实利用脚本。
 */

import org.springframework.web.bind.annotation.ModelAttribute;




public class ModelAttrDeepBindBx {

    // 嵌套 POJO：授权字段 role 位于内层
    public static class UserPrefs {
        private String role; // 危险字段：框架按 form.role 属性路径隐式绑定
        public String getRole() { return role; }
        public void setRole(String v) { this.role = v; }
    }

    public static class AccountForm {
        private String username;
        private UserPrefs prefs = new UserPrefs(); // 嵌套对象，支持 form.prefs.role 深度绑定
        public String getUsername() { return username; }
        public void setUsername(String v) { this.username = v; }
        public UserPrefs getPrefs() { return prefs; }
        public void setPrefs(UserPrefs v) { this.prefs = v; }
    }

    





    public String handle(@ModelAttribute AccountForm form) {
        // 框架语义：form.prefs.role 已被攻击者表单值（form.prefs.role=ADMIN）污染
        /*ANCHOR_1*/
        if ("ADMIN".equals(form.getPrefs().getRole())) {
            return "granted-admin"; // 深度绑定污染的 role 驱动越权授权
        }
        return "user";
    }

    public static void main(String[] args) {
        AccountForm f = new AccountForm();
        f.getPrefs().setRole("ADMIN"); // 模拟攻击者深度绑定 form.prefs.role=ADMIN
        System.out.println(new ModelAttrDeepBindBx().handle(f));
    }
}
