
package blinded;






public class RoleElevationServiceBy {

    
    public String elevate(String currentRole) {
        /*ANCHOR_1*/
        return "ADMIN".equals(currentRole) ? "ADMIN" : "DENIED"; // 非 ADMIN 不可提权
    }
}
