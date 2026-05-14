#include <iostream>
#include <string>
#include <vector>

int main(int argc, char* argv[]) {
    std::cout << "Writer Linux Native Skeleton" << std::endl;

    std::vector<std::string> args;
    for (int i = 1; i < argc; ++i) {
        args.push_back(argv[i]);
    }

    bool sync_requested = false;
    bool dry_run = false;

    for (const auto& arg : args) {
        if (arg == "--sync") {
            sync_requested = true;
        } else if (arg == "--dry-run") {
            dry_run = true;
        }
    }

    if (sync_requested) {
        if (dry_run) {
            std::cout << "[Stub] Calling Rust Facade: perform_sync_dry_run()" << std::endl;
        } else {
            std::cout << "[Stub] Calling Rust Facade: perform_sync()" << std::endl;
        }
    }

    return 0;
}
