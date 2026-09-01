
package blinded;


















public class ConditionalAuthzBypass {

    private boolean maintenanceMode;
    private boolean skipAuthWindow;

    public ConditionalAuthzBypass(boolean maintenanceMode, boolean skipAuthWindow) {
        this.maintenanceMode = maintenanceMode;
        this.skipAuthWindow = skipAuthWindow;
    }

    


    public Object adminResource(String callerRole) {
        /*ANCHOR_1*/
        if (maintenanceMode && skipAuthWindow) {
            return doAdminAction(); // 双条件满足：低权限可达（越权）
        }
        if (!"ADMIN".equals(callerRole)) {
            return "DENIED";
        }
        return doAdminAction();
    }

    private Object doAdminAction() {
        System.out.println("[abstract admin action] executed");
        return "OK";
    }
}
