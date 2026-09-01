package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RestController;

import java.io.ByteArrayInputStream;
import java.io.ObjectInputStream;
import java.util.Set;




















@RestController
public class ProfileControllerBy {

    
    private static final Set<Class<?>> ALLOWED_TYPES = Set.of(ProfileRecord.class);

    private final MaskedAuditLog auditLog = new MaskedAuditLog();

    


    @PostMapping("/benchmark/multibx/profile/by")
    public String fetchProfile(@RequestBody byte[] payload, @RequestBody CallerPrincipal caller) throws Exception {
        // 白名单反序列化：非白名单类型抛异常（链条在环2 起点被打断）
        ProfileRecord data = deserialize(payload);
        if (data == null) {
            return "REJECTED";
        }
        // 归属校验：仅允许读取当前调用者自己的资料
        /*ANCHOR_1*/
        if (!caller.owns(data)) {
            return "DENIED";
        }
        // 日志脱敏：仅记录掩码后的 id，不泄漏原始 userId
        auditLog.logAccessMasked(data.getUserId());
        return readProfile(data.getUserId());
    }

    


    static ProfileRecord deserialize(byte[] payload) throws Exception {
        ObjectInputStream ois = new ObjectInputStream(new ByteArrayInputStream(payload));
        Object obj = ois.readObject();
        ois.close();
        if (!ALLOWED_TYPES.contains(obj.getClass())) {
            throw new SecurityException("forbidden type: " + obj.getClass());
        }
        return (ProfileRecord) obj;
    }

    
    static String readProfile(String userId) {
        System.out.println("[profile-read-by] userId=" + userId);
        return "profile-data:" + userId;
    }

    
    static class MaskedAuditLog {
        
        void logAccessMasked(String userId) {
            String masked = userId == null || userId.length() < 2 ? "****"
                    : userId.substring(0, 1) + "***";
            System.out.println("[audit-by] access by user=" + masked);
        }
    }

    
    static class ProfileRecord {
        private String userId;
        String getUserId() { return userId; }
        void setUserId(String userId) { this.userId = userId; }
    }

    
    static class CallerPrincipal {
        private final String currentUserId;
        CallerPrincipal(String currentUserId) { this.currentUserId = currentUserId; }
        boolean owns(ProfileRecord data) {
            return currentUserId.equals(data.getUserId());
        }
    }
}
