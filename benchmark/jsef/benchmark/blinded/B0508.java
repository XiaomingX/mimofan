package blinded;

import java.util.Arrays;

/*
 * JSEF-Benchmark L4 — HTTP Parameter Pollution 导致越权绑定角色 (CWE-915)
 *
 * 难度：L4（框架语义 / 跨节点）。Spring 对重复参数 roles=USER&roles=ADMIN
 * 解析为数组；bx 侧取“全部/最后一个”绑定，使普通用户借 HPP 提权为 ADMIN。
 *
 * 安全底线：仅 localhost 演示语义，不提供真实利用载荷。
 * 修复要点（HppRoleBindingBy.java）：单一取值、忽略重复参数，或固定为 USER。
 */
public class HppRoleBindingBx {

    
    static class Req {
        String[] getParams(String name) {
            // 攻击者构造 roles=USER&roles=ADMIN → ["USER","ADMIN"]
            return new String[]{"USER", "ADMIN"};
        }
    }

    
    static class Account {
        void setRoles(java.util.List<String> roles) {
            System.out.println("[bind-roles] " + roles);
        }
    }

    /*ANCHOR_1*/
    public void bind(Req req, Account acc) {
        // 取参数行：重复参数被解析为数组，含攻击者注入的 ADMIN
        String[] roles = req.getParams("roles");
        // setRoles 行：将含 ADMIN 的列表整体绑定 → 越权提权
        acc.setRoles(Arrays.asList(roles));
    }
}
