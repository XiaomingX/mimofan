package blinded;

import java.util.Arrays;

/*
 * JSEF-Benchmark L4 — HPP 角色绑定修复 (CWE-915) expect=BX
 *
 * sec 侧：单一取值 req.getParam("roles")（仅首个），忽略重复参数，
 * 或固定为 USER，避免攻击者借重复参数注入 ADMIN。
 *
 * 安全底线：按实现判定为安全。
 */
public class HppRoleBindingBy {

    static class Req {
        String getParam(String name) {
            return "USER"; // 仅取首个 / 忽略重复参数
        }
    }

    static class Account {
        void setRoles(java.util.List<String> roles) {
            System.out.println("[bind-roles] " + roles);
        }
    }

    /*ANCHOR_1*/
    public void bind(Req req, Account acc) {
        // 单一取值，重复参数被忽略
        String role = req.getParam("roles");
        acc.setRoles(Arrays.asList(role));
    }
}
