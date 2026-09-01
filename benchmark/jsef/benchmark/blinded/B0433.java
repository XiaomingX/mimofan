
package blinded;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestHeader;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;
















@RestController
public class TenantInvoiceController {

    private final TenantContext tenantContext;
    private final TenantDataService dataService;

    public TenantInvoiceController(TenantContext tenantContext,
                                   TenantDataService dataService) {
        this.tenantContext = tenantContext;
        this.dataService = dataService;
    }

    @GetMapping("/api/v1/invoices")
    public String getInvoice(@RequestHeader("X-Tenant-Id") String tenantId,
                             @RequestParam("invoiceId") String invoiceId) {
        // 入口：tenantId 来自请求头，invoiceId 来自请求参数（二者均外部可控）
        tenantContext.setTenant(tenantId); // 注入租户上下文（隔离边界起点）
        // 缺陷：查询直接用请求参数 invoiceId，未与 tenantContext.tenantId 做归属校验
        /*ANCHOR_1*/
        return dataService.loadInvoice(invoiceId); // 可越权读取其他租户发票
    }
}
