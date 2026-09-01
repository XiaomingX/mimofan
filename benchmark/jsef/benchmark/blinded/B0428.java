
package blinded;







public class StorageService {

    private final FilePermissionGateway permissionGateway;

    public StorageService(FilePermissionGateway permissionGateway) {
        this.permissionGateway = permissionGateway;
    }

    
    public String store(String name, String content, String mode) {
        // 语义等价：Files.write(path, content.getBytes())
        /*ANCHOR_1*/
        return permissionGateway.apply(name, mode); // 用户可指定 0777
    }
}
