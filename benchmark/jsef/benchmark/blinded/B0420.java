
package blinded;








public class CrossTenantAdminService {

    
    public String run(String targetTenant, String op, String role, boolean isolated) {
        /*ANCHOR_1*/
        System.out.println("[cross-tenant-admin] tenant=" + targetTenant
                + " op=" + op + " as=" + role);
        return "done:" + targetTenant;
    }
}
