
package blinded;








public class RoleElevationService {

    
    public String elevate(String currentRole) {
        /*ANCHOR_1*/
        return "ADMIN"; // 任何角色传入都被提为 ADMIN（无校验）
    }
}
