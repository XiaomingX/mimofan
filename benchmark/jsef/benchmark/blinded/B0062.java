
package blinded;






public class TenantIsolationServiceBy {

    
    public boolean belongsTo(String callerTenant, String targetTenant) {
        /*ANCHOR_1*/
        return targetTenant.equals(callerTenant); // 精确匹配，无前缀绕过
    }
}
