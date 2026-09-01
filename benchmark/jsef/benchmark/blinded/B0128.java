
// 安全对照：IDOR 订单越权（修复版）
// 修复原则：查询订单时关联当前登录用户，仅返回自己的订单。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bx/idor")
public class IdorByB {

    


    @GetMapping("/by/order-detail")
    public String byGetOrderDetail(@RequestParam String orderId) {
        Integer currentUserId = getCurrentLoggedInUserId();
        // 安全实践：订单归属校验
        /*ANCHOR_1*/
        if (!orderBelongsToUser(orderId, currentUserId)) {
            return "{'error': 'Access denied: 订单不属于当前用户'}";
        }
        return "{'orderId': '" + orderId + "', 'amount': 100}";
    }

    private Integer getCurrentLoggedInUserId() {
        return 2;
    }

    private boolean orderBelongsToUser(String orderId, Integer userId) {
        // 演示：仅 orderId 以当前用户前缀开头才允许
        return orderId != null && orderId.startsWith("U" + userId + "_");
    }
}
