
package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;
















@RestController
public class FileUploadController {

    private final StorageService storageService;

    public FileUploadController(StorageService storageService) {
        this.storageService = storageService;
    }

    @PostMapping("/api/v1/files/upload")
    public String upload(@RequestParam("name") String name,
                         @RequestParam("content") String content,
                         @RequestParam("mode") String mode) {
        // 入口：mode（权限八进制串）来自外部请求参数（source）
        // 缺陷：未校验 mode 是否在安全白名单（如仅 0644），直接下发
        /*ANCHOR_1*/
        return storageService.store(name, content, mode);
    }
}
