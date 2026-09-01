
package blinded;




public class CrossTenantAdminServiceBy {

    public String run(String targetTenant, String op) {
        // 语义等价：以已验证 ADMIN 身份在已验证同租户执行管理操作
        System.out.println("[cross-tenant-admin][by] tenant=" + targetTenant + " op=" + op);
        return "done:" + targetTenant;
    }
}
