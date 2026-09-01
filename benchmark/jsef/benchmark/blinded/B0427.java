
package blinded;








public class RoleService {

    private final UserStore userStore;

    public RoleService(UserStore userStore) {
        this.userStore = userStore;
    }

    
    public String updateRole(String userId, String newRole) {
        /*ANCHOR_1*/
        return userStore.persistRole(userId, newRole); // 任意用户均可提权至 ADMIN
    }
}
