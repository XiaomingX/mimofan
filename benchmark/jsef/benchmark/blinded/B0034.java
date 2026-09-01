/*
 * JSEF Benchmark 安全样本 — 业务流程绕过（A01，CWE-840，L4）
 * BX 版：查询订单前先校验状态机前置条件（status==PAID），未支付则拒绝。
 * 测试点：强 SAST/LLM 应识别状态机校验已存在而不报（TN）。
 * 运行态需 JSEF 依赖；独立 benchmark 源文件，不强求编译。
 */
public class WorkflowBypassBy {

    static final class Order { final String id; final String status;   // PENDING / PAID
        Order(String id, String status){ this.id=id; this.status=status; } }

    


    static Order queryOrder(Order order) {
        // 状态机校验：阻断跳过支付步骤的越权访问
        /*ANCHOR_1*/
        if (!"PAID".equals(order.status)) {
            throw new SecurityException("403 forbidden: order not paid");
        }
        return order;   // 仅已支付订单可被查询
    }
}
