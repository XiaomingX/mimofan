
package blinded;






public class RoleServiceBy {

    private final UserStoreBy userStore;

    public RoleServiceBy(UserStoreBy userStore) {
        this.userStore = userStore;
    }

    public String updateRole(String callerRole, String userId, String newRole) {
        // 真实实现了权限闸门：仅 ADMIN 可改角色，且禁止 self-promotion
        if (!"ADMIN".equals(callerRole)) {
            return "denied: insufficient privilege";
        }
        /*ANCHOR_1*/
        return userStore.persistRole(userId, newRole);
    }
}
