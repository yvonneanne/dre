"""
rules for creating oci images from python binaries
"""

load("@rules_oci//oci:defs.bzl", "oci_image", "oci_push", "oci_tarball")
load("@rules_pkg//:pkg.bzl", "pkg_tar")

def python_oci_image_rules(name, src, base_image = "@distroless_python3"):
    """macro for creating oci image from python binary

    Args:
        name: not used
        src: label of py binary to be put in the OCI image
        base_image: base image for building py binaries
    """
    binary = native.package_relative_label(src)
    tar_rule_name = "{}_layer".format(binary.name)
    pkg_tar(
        name = tar_rule_name,
        srcs = [binary],
        include_runfiles = True,
        strip_prefix = "/",
        remap_paths = {
            "k8s/{}/{}.py".format(binary.name, binary.name): "{}.runfiles/dre/k8s/{}/{}.py".format(binary.name, binary.name, binary.name),
            "k8s/{}/{}".format(binary.name, binary.name): "{}".format(binary.name),
            "external": "{}.runfiles".format(binary.name),
        }
    )

    image_rule_name = "{}-image".format(binary.name)
    oci_image(
        name = image_rule_name,
        # Consider using even more minimalistic docker image since we're using static compile
        base = base_image,
        entrypoint = ["python3", "/{}".format(binary.name)],
        tars = [tar_rule_name],
        env = {
            "GIT_PYTHON_REFRESH": "quiet"
        }
    )

    tarball_name = "{}-tarball".format(binary.name)
    oci_tarball(
        name = tarball_name,
        image = image_rule_name,
        repo_tags = ["localhost/{}:latest".format(binary.name)]
    )

    oci_push(
        name = "push_image",
        image = image_rule_name,
        repository = "ghcr.io/dfinity/dre/{}".format(binary.name),
    )
