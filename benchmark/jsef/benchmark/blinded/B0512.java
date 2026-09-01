
// 漏洞样本：IDOR——订单详情越权查看（未验证订单归属）
// 漏洞点：仅通过 orderId 查询订单，未验证该订单是否属于当前登录用户。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bz/idor")
public class IdorBxB {

    


    @GetMapping("/unby/order-detail")
    public String unbyGetOrderDetail(@RequestParam String orderId) {
        // 危险实践：未验证订单归属当前用户
        /*ANCHOR_1*/
        return "{'orderId': '" + orderId + "', 'amount': 100, 'userInfo': 'sensitive data'}";
    }
}
