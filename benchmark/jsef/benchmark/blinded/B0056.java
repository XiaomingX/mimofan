
package blinded;






public class StorageServiceBy {

    private final FilePermissionGatewayBy permissionGateway;

    public StorageServiceBy(FilePermissionGatewayBy permissionGateway) {
        this.permissionGateway = permissionGateway;
    }

    public String store(String name, String content, String requestedMode) {
        // 真实实现降权：忽略用户请求，固定为安全权限
        String byMode = "0644"; // 仅属主可写，其他只读
        /*ANCHOR_1*/
        return permissionGateway.apply(name, byMode);
    }
}
